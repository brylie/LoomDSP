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
/// of [`LOOPBACK_BUFFER`] frames at `rate`, captures the loopback's input side, and stops
/// the stream with `stop` after a fade-out request. `stop` returns true if
/// the stream stopped on the engine's silence, false if it was cut at the
/// limit; the answer is printed, so a failed check says which it was.
/// Returns the captured left channel and the sample rate, or `None` when
/// `LOOM_LOOPBACK` is not set.
fn capture_a_stop(rate: u32, stop: impl FnOnce(RunningAudio) -> bool) -> Option<(Vec<f32>, u32)> {
    capture_a_stop_with_input_rate(rate, rate, stop).map(|(samples, rate, _, _)| (samples, rate))
}

/// Captures a stream while allowing the input and output to request different rates.
fn capture_a_stop_with_input_rate(
    rate: u32,
    input_rate: u32,
    stop: impl FnOnce(RunningAudio) -> bool,
) -> Option<(Vec<f32>, u32, StatsSnapshot, CaptureReport)> {
    // Two tests playing into one loopback at once would hear each
    // other, so one capture runs at a time. A failed test poisons the
    // lock; the next one still runs.
    static LOOPBACK: Mutex<()> = Mutex::new(());
    let _one_at_a_time = LOOPBACK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let loopback = env_or_skip("LOOM_LOOPBACK", "part of a loopback device's name")?;
    let options = AudioOptions {
        device: Some(loopback),
        exact: false,
        sample_rate: Some(rate),
        buffer_size: Some(LOOPBACK_BUFFER),
    };
    let mut output = or_skip(AudioOutput::open(&options))?;
    let rate = output.sample_rate();
    let dir = record::TestDir::new(&format!("capture-{:?}", std::thread::current().id()));
    let path = dir.0.join("capture.wav");
    let rendered_path = dir.0.join("rendered.wav");
    let recorder = Recorder::start(&rendered_path, rate).expect("create engine recording");
    output.record(&recorder).expect("record at the output rate");
    let input = AudioOptions {
        sample_rate: Some(input_rate),
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
    assert_eq!(recorder.dropped(), 0, "engine recording kept every frame");
    assert!(recorder.close().is_none(), "engine recording closed cleanly");
    // A dropout with no error on either side happened in the device
    // (for example the PipeWire graph), not in LoomDSP's callbacks.
    let s = stats.snapshot();
    eprintln!(
        "output: {} callbacks, {} overruns, {} stream errors, {} start overloads, rate switched {}, {} start restarts; capture: {} frames, {} stream errors",
        s.callbacks,
        s.overruns,
        s.stream_errors,
        s.start_overloads,
        s.start_rate_switched,
        s.start_restarts,
        report.frames,
        report.stream_errors
    );
    assert_eq!(report.dropped, 0, "no frame was dropped");

    let mut rendered = hound::WavReader::open(&rendered_path).unwrap();
    let rendered: Vec<f32> = rendered.samples::<f32>().step_by(2).map(Result::unwrap).collect();
    let rendered_level = rendered.iter().fold(0.0f32, |maximum, value| maximum.max(value.abs()));
    let rendered_step = start_step(&rendered, rendered_level);
    eprintln!("engine start: level {rendered_level}, largest step {rendered_step}");
    assert!(rendered_level > 0.01, "the engine rendered sound");
    assert!(
        rendered_step <= fade_step(rendered_level, rate),
        "the engine rendered its fade-in"
    );

    let mut reader = hound::WavReader::open(&path).unwrap();
    let left = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
    Some((left, rate, s, report))
}

/// The level the capture holds, and the largest step of the stop: from
/// the last sample at that level on, where the fade must be. The
/// loopback's volume scales the level, so checks are relative to it.
///
/// A step inside the stream before that is a dropout, not the stop: on
/// a busy CI runner, PipeWire sometimes delivers one 512-frame quantum
/// of silence and then the level again (#361). It is printed, with
/// where it is, but does not count as the stop's step.
///
/// Why it is likely not LoomDSP's: in the failures, the stream stopped
/// on the engine's silence, the gap came about 160 ms before the
/// fade-out was asked for, and two failures had it at the same frame
/// (24063) of the first try on a fresh sink. That fits the PipeWire
/// graph settling on the new stream's 4096-frame latency more than a
/// callback that ran late. The overrun and stream error counts printed
/// with each capture tell the two apart if it comes back.
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

/// Measures the largest step from capture start through the first full level.
fn start_step(samples: &[f32], level: f32) -> f32 {
    let held = samples.iter().position(|sample| sample.abs() >= level).unwrap_or(0);
    largest_step(samples, 0, held).1
}

/// A missing fade at stream start must be visible even when the stop fades.
#[test]
fn start_step_detects_a_missing_fade() {
    let mut faded = vec![0.0; 100];
    faded.extend((0..=480).map(|i| 0.5 * i as f32 / 480.0));
    assert!(start_step(&faded, 0.5) <= fade_step(0.5, 48_000));
    assert_eq!(start_step(&[0.0, 0.5, 0.5], 0.5), 0.5);
}

/// Checks that the loopback receives both fades at both rates.
#[test]
#[ignore = "needs a loopback audio device"]
fn a_start_with_large_buffers_fades_in_on_the_device() {
    for _ in 0..STOPS {
        // Switching each time also exercises the rate-change path.
        for requested_rate in [48_000, 96_000] {
            let Some((samples, rate)) = capture_a_stop(requested_rate, |audio| audio.stop(STOP_LIMIT)) else {
                return;
            };
            assert_eq!(rate, requested_rate, "loopback must use the requested rate");
            let (level, stop_step) = level_and_step(&samples);
            let step = start_step(&samples, level);
            let limit = fade_step(level, rate);
            assert!(
                step <= limit,
                "start step {step} exceeds {limit} at {rate} Hz (level {level})"
            );
            assert!(
                stop_step <= limit,
                "stop step {stop_step} exceeds {limit} at {rate} Hz (level {level})"
            );
        }
    }
}

/// A captured rate-switch restart must actually occur and fade in on the device.
#[test]
#[ignore = "needs a loopback device and a late rate-switch notice"]
fn a_rate_switch_restart_reaches_the_loopback() {
    // The capture starts at 48 kHz, before the output requests 96 kHz, so
    // opening the capture cannot silently perform the switch first.
    let Some((samples, rate, stats, capture)) =
        capture_a_stop_with_input_rate(96_000, 48_000, |audio| audio.stop(STOP_LIMIT))
    else {
        return;
    };
    assert_eq!(rate, 96_000);
    assert!(stats.start_rate_switched, "no output rate switch was observed");
    assert!(stats.start_restarts > 0, "no late notice restarted the output stream");
    assert_eq!(capture.stream_errors, 0, "capture failed during the rate switch");
    let (level, stop_step) = level_and_step(&samples);
    // The input was requested at 48 kHz. Use its larger fade step if the
    // backend resamples it while the output is at 96 kHz.
    let limit = fade_step(level, 48_000);
    assert!(start_step(&samples, level) <= limit, "the captured start faded in");
    assert!(stop_step <= limit, "the captured stop faded out");
}

/// A deliberately paused real stream fades when the host resumes it.
#[test]
#[ignore = "needs a loopback audio device"]
fn a_manually_resumed_stream_fades_on_the_loopback() {
    let Some(device_name) = env_or_skip("LOOM_LOOPBACK", "part of a loopback device's name") else {
        return;
    };
    for rate in [48_000, 96_000] {
        let options = AudioOptions {
            device: Some(device_name.clone()),
            exact: false,
            sample_rate: Some(rate),
            buffer_size: Some(4096),
        };
        let Some(output) = or_skip(AudioOutput::open(&options)) else {
            return;
        };
        let AudioOutput {
            kind: Kind::Device { device, config },
            ..
        } = output
        else {
            panic!("the loopback is a device");
        };
        let dir = record::TestDir::new(&format!("resume-{rate}"));
        let path = dir.0.join("capture.wav");
        let Some(capture) = or_skip(Capture::start(&options, &path)) else {
            return;
        };
        let started = Instant::now();
        while capture.frames() == 0 {
            assert!(started.elapsed() < Duration::from_secs(10), "capture started");
            std::thread::sleep(Duration::from_millis(1));
        }
        let engine_config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(rate as f32)
        };
        let mut parts = create_engine(loom_modules::registry(), engine_config);
        let constant = parts.controller.add_module("loom.math.constant").unwrap();
        let out = parts.controller.add_module("loom.io.audio-out").unwrap();
        parts.controller.set_param_named(constant, "value", 0.5).unwrap();
        parts.controller.connect_named(constant, "out", out, "in").unwrap();
        parts.controller.commit().unwrap();
        let mut restart = parts.engine.restart_fade_sender();
        let stats = Arc::new(CallbackStats::default());
        let raw = Box::into_raw(Box::new(Render::new(
            parts.engine,
            usize::from(config.channels),
            rate,
            stats,
            None,
        )));
        let ptr = RenderPtr(raw);
        let stream = match device.build_output_stream::<f32, _, _>(
            config,
            move |data, _| {
                // SAFETY: the StreamHandle owns the render until the callback stops.
                unsafe { ptr.render(data) }
            },
            |_| {},
            None,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                // SAFETY: stream creation failed, so no callback can access the render.
                unsafe { drop(Box::from_raw(raw)) };
                panic!("output stream: {error}");
            }
        };
        let handle = StreamHandle {
            stream: Some(stream),
            render: raw,
        };
        let stream = handle.stream.as_ref().unwrap();
        stream.play().unwrap();
        std::thread::sleep(Duration::from_millis(800));
        stream.pause().unwrap();
        // Wait beyond the device and capture buffers, so the captured region
        // beginning at this marker contains the restart rather than old audio.
        std::thread::sleep(Duration::from_millis(500));
        let resume_at = capture.frames() as usize;
        restart_stream(&mut restart, || stream.play().map_err(audio_error)).unwrap();
        std::thread::sleep(Duration::from_millis(700));
        let end = capture.frames() as usize;
        parts.controller.fade_out().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        drop(handle);
        let report = capture.stop().unwrap();
        assert_eq!((report.dropped, report.stream_errors), (0, 0));
        let mut reader = hound::WavReader::open(path).unwrap();
        let left: Vec<f32> = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
        let resumed = &left[resume_at..end];
        let level = resumed.iter().map(|sample| sample.abs()).fold(0.0_f32, f32::max);
        assert!(level > 0.01, "the resumed device delivered sound");
        let step = start_step(resumed, level);
        eprintln!(
            "{rate} Hz: manually resumed, level {level}, start step {step}, {} captured frames",
            resumed.len()
        );
        assert!(step <= fade_step(level, rate), "the device received the restart fade");
    }
}

/// Measures whether a real output rate change triggers a startup restart.
#[test]
#[ignore = "needs a loopback audio device"]
fn rate_switch_reports_startup_restarts() {
    let Some(device) = env_or_skip("LOOM_LOOPBACK", "part of a loopback device's name") else {
        return;
    };
    for rate in [48_000, 96_000] {
        let options = AudioOptions {
            device: Some(device.clone()),
            exact: false,
            sample_rate: Some(rate),
            buffer_size: Some(4096),
        };
        let Some(output) = or_skip(AudioOutput::open(&options)) else {
            return;
        };
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(rate as f32)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let constant = parts.controller.add_module("loom.math.constant").unwrap();
        let out = parts.controller.add_module("loom.io.audio-out").unwrap();
        parts.controller.set_param_named(constant, "value", 0.5).unwrap();
        parts.controller.connect_named(constant, "out", out, "in").unwrap();
        parts.controller.commit().unwrap();
        let audio = output.start(parts.engine).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let stats = audio.stats().snapshot();
        eprintln!(
            "{rate} Hz: switched {}, restarts {}, callbacks {}, stream errors {}",
            stats.start_rate_switched, stats.start_restarts, stats.callbacks, stats.stream_errors
        );
        if cfg!(target_os = "macos") && rate == 96_000 {
            assert!(
                stats.start_rate_switched,
                "the 48 to 96 kHz transition reached the host"
            );
        }
        assert!(stats.callbacks > 0);
        parts.controller.fade_out().unwrap();
        assert!(audio.stop(STOP_LIMIT));
    }
}

/// The actual input stream carries loopback sound through Audio In.
#[test]
#[ignore = "needs a loopback audio device"]
fn audio_input_receives_sound_from_a_loopback_device() {
    let Some(device) = env_or_skip("LOOM_LOOPBACK", "part of a loopback device's name") else {
        return;
    };
    let options = AudioOptions {
        device: Some(device),
        exact: false,
        sample_rate: Some(48_000),
        buffer_size: Some(256),
    };
    let Some(output) = or_skip(AudioOutput::open(&options)) else {
        return;
    };
    let Some((input, reader)) = or_skip(AudioInput::start(&options, 256)) else {
        return;
    };
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut source = create_engine(loom_modules::registry(), config);
    let constant = source.controller.add_module("loom.math.constant").unwrap();
    let output_module = source.controller.add_module("loom.io.audio-out").unwrap();
    source.controller.set_param_named(constant, "value", 0.5).unwrap();
    source
        .controller
        .connect_named(constant, "out", output_module, "in")
        .unwrap();
    source.controller.commit().unwrap();
    let audio = output.start(source.engine).unwrap();

    let mut receiver = create_engine(loom_modules::registry(), config);
    receiver.controller.set_audio_input(Some(reader)).unwrap();
    let input_module = receiver.controller.add_module("loom.io.audio-in").unwrap();
    let receiver_output = receiver.controller.add_module("loom.io.audio-out").unwrap();
    receiver
        .controller
        .connect_named(input_module, "out", receiver_output, "in")
        .unwrap();
    receiver.controller.commit().unwrap();
    let mut left = [0.0; 256];
    let mut right = [0.0; 256];
    let started = Instant::now();
    let mut peak = [0.0_f32; 2];
    let mut active = false;
    while started.elapsed() < Duration::from_secs(3) {
        receiver.engine.process(&mut left, &mut right);
        active |= receiver.controller.display(input_module, 0) == Some(1.0);
        for (&l, &r) in left.iter().zip(&right) {
            peak[0] = peak[0].max(l.abs());
            peak[1] = peak[1].max(r.abs());
        }
        std::thread::sleep(Duration::from_secs_f64(256.0 / 48_000.0));
    }
    eprintln!(
        "input {}: {} channels, {} callbacks, peaks {peak:?}",
        input.name(),
        input.channels(),
        input.status().callbacks()
    );
    assert!(input.is_live());
    assert!(input.status().callbacks() > 0);
    assert!(
        peak[0] > 0.01 && peak[1] > 0.01,
        "both channels received the loopback signal"
    );
    assert!(active, "the input activity light followed the signal");
    source.controller.fade_out().unwrap();
    assert!(audio.stop(STOP_LIMIT));
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
        let Some((samples, rate)) = capture_a_stop(LOOPBACK_RATE, |audio| audio.stop(STOP_LIMIT)) else {
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
        let Some((samples, rate)) = capture_a_stop(LOOPBACK_RATE, |audio| audio.stop_within(|_| STOP_LIMIT)) else {
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
