//! Checks on a real audio device. They need hardware, so they are ignored
//! by default: run them with
//! `cargo test -p loom-host-desktop -- --ignored --test-threads=1`, so no
//! two of them use a device at once
//! (set `LOOM_DEVICE` to part of a device name; the default device
//! otherwise). Each stream is silent, except in the capture tests, which
//! play a DC offset into a loopback device (`LOOM_LOOPBACK`) and are
//! skipped when it is not set.
//!
//! In a file of their own, so coverage, measured on machines with no
//! audio device, leaves them out.

use super::*;
use loom_core::{EngineConfig, OutputSafety, create_engine};

/// Plays a silent engine on `options` for 0.7 s and returns the
/// callbacks the device made.
fn callbacks(options: &AudioOptions) -> u64 {
    let output = AudioOutput::open(options).unwrap();
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
    made
}

/// Plays `examples/dev/check-dc.ron` (a Constant of 0.5, output safety
/// Off) into the loopback device named by `LOOM_LOOPBACK` with buffers
/// of [`LOOPBACK_BUFFER`] frames at [`LOOPBACK_RATE`], captures the loopback's input side, and stops
/// the stream with `stop` after a fade-out request. Returns the
/// captured left channel and the sample rate, or `None` when
/// `LOOM_LOOPBACK` is not set.
fn capture_a_stop(stop: impl FnOnce(RunningAudio)) -> Option<(Vec<f32>, u32)> {
    // Two tests playing into one loopback at once would hear each
    // other, so one capture runs at a time. A failed test poisons the
    // lock; the next one still runs.
    static LOOPBACK: Mutex<()> = Mutex::new(());
    let _one_at_a_time = LOOPBACK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(loopback) = std::env::var("LOOM_LOOPBACK") else {
        eprintln!("skipped: set LOOM_LOOPBACK to part of a loopback device's name, such as BlackHole");
        return None;
    };
    let options = AudioOptions {
        device: Some(loopback),
        exact: false,
        sample_rate: Some(LOOPBACK_RATE),
        buffer_size: Some(LOOPBACK_BUFFER),
    };
    let output = AudioOutput::open(&options).unwrap();
    let rate = output.sample_rate();
    let dir = record::TestDir::new(&format!("capture-{:?}", std::thread::current().id()));
    let path = dir.0.join("capture.wav");
    let input = AudioOptions {
        sample_rate: Some(rate),
        buffer_size: None,
        ..options
    };
    let capture = Capture::start(&input, &path).unwrap();

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
    std::thread::sleep(Duration::from_millis(600));
    parts.controller.fade_out().unwrap();
    stop(audio);
    // Long enough for the device to play out what it was handed.
    std::thread::sleep(Duration::from_millis(400));
    let report = capture.stop().unwrap();
    assert_eq!(report.dropped, 0, "no frame was dropped");

    let mut reader = hound::WavReader::open(&path).unwrap();
    let left = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
    Some((left, rate))
}

/// The level the capture holds, and its largest step between two
/// neighbouring samples from the first sample at that level on: the
/// stop, not the start. The loopback's volume scales the level, so
/// checks are relative to it.
fn level_and_step(samples: &[f32]) -> (f32, f32) {
    let level = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let held = samples.iter().position(|x| x.abs() >= level).unwrap_or(0);
    let step = samples[held..]
        .windows(2)
        .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    eprintln!("captured level {level}, largest step {step}");
    assert!(
        level > 0.01,
        "the loopback delivered sound (level {level}). On macOS the terminal needs microphone access."
    );
    (level, step)
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
        let Some((samples, rate)) = capture_a_stop(|audio| {
            audio.stop(STOP_LIMIT);
        }) else {
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
        let Some((samples, rate)) = capture_a_stop(|audio| {
            audio.stop_within(|_| STOP_LIMIT);
        }) else {
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
    let device = std::env::var("LOOM_DEVICE").ok();
    for rate in [44_100, 48_000, 96_000, 44_100] {
        let options = AudioOptions {
            device: device.clone(),
            exact: false,
            sample_rate: Some(rate),
            buffer_size: Some(4096),
        };
        assert!(callbacks(&options) > 0, "the stream at {rate} Hz plays");
    }
}
