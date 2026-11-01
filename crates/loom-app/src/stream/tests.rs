use super::*;
use loom_core::{EngineConfig, create_engine};
use loom_host_desktop::AudioOutput;

/// A new engine, for a stream to play or keep idle.
fn engine() -> Engine {
    create_engine(loom_modules::registry(), EngineConfig::default()).engine
}

/// A stream with nothing playing and no idle engine.
fn empty() -> Stream {
    Stream::new(
        None,
        None,
        "no audio".into(),
        48_000.0,
        Playback::Device,
        OutputSafety::LowLatency,
    )
}

/// `--no-audio` wins over `--fake-audio`, and either means no device.
#[test]
fn the_flags_choose_the_playback() {
    assert_eq!(Playback::from_flags(false, false), Playback::Device);
    assert_eq!(Playback::from_flags(false, true), Playback::Fake);
    assert_eq!(Playback::from_flags(true, true), Playback::Off);
    let mut stream = empty();
    assert!(!stream.without_device());
    stream.set_playback(Playback::Fake);
    assert!(stream.without_device());
}

/// Setting the mode it already has reports no change, so the app neither
/// saves nor tells the engine.
#[test]
fn the_same_safety_mode_is_no_change() {
    let mut stream = empty();
    assert!(!stream.set_output_safety(OutputSafety::LowLatency));
    assert!(stream.set_output_safety(OutputSafety::Clean));
    assert_eq!(stream.output_safety(), OutputSafety::Clean);
}

/// Callbacks that stop growing for longer than STALL count as a stall. A
/// watcher's count is timed from when the watcher saw it grow.
#[test]
fn a_stall_is_timed_from_the_last_growth() {
    let mut stream = empty();
    let start = Instant::now();
    assert!(stream.observe_callbacks(5, start));
    assert!(!stream.observe_callbacks(5, start + STALL), "the count did not grow");
    assert!(!stream.stalled(start + STALL));
    assert!(stream.stalled(start + STALL + Duration::from_millis(1)));

    let later = start + Duration::from_secs(5);
    stream.watcher_saw(6, later);
    assert_eq!(stream.callbacks(), (6, later));
    assert!(!stream.stalled(later + STALL));
    stream.watcher_saw(4, later + Duration::from_secs(9));
    assert_eq!(stream.callbacks(), (6, later), "an older count changes nothing");
}

/// A started stream plays with fresh counters; stopping it clears them.
/// A refused one keeps the engine idle and says why.
#[test]
fn starting_and_stopping_keep_the_engine_somewhere() {
    let mut stream = empty();
    stream.keep_idle(engine());
    assert!(stream.has_engine() && stream.is_idle());

    stream.prepare(44_100.0);
    assert!(!stream.is_idle(), "the old idle engine goes");
    assert_eq!(stream.sample_rate(), 44_100.0);
    let audio = AudioOutput::clock(44_100, 64).start(engine()).unwrap();
    stream.set_load(0.5, 0.9);
    stream.started(audio, "Clock at 44100 Hz".into());
    assert!(stream.is_playing());
    assert!(stream.stats().is_some());
    assert_eq!(stream.load(), (0.0, 0.0), "a new stream starts with no load");
    assert_eq!(stream.label(), "Clock at 44100 Hz");

    let running = stream.take_running().expect("it was playing");
    running.stop_immediate();
    assert!(!stream.is_playing());
    assert!(stream.stats().is_none());

    stream.refused(engine(), "the device is busy");
    assert!(stream.is_idle());
    assert_eq!(stream.label(), "no audio (the device is busy)");
}
