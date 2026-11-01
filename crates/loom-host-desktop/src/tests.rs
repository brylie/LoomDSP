use super::*;
use loom_core::{EngineConfig, EngineParts, OutputSafety, create_engine};

/// Two inputs with the same name are told apart by order.
#[test]
fn same_names_are_numbered_by_order() {
    let names = ["Pad", "Keys", "Pad", "Pad"].map(String::from);
    assert_eq!(unique_names(&names), ["Pad", "Keys", "Pad (2)", "Pad (3)"]);
    assert_eq!(find_input(&names, "Pad (2)").unwrap(), 2);
    assert_eq!(find_input(&names, "Keys").unwrap(), 1);
    let missing = find_input(&names, "Drums").unwrap_err();
    assert!(missing.to_string().contains("Drums"), "{missing}");
    let ids = ["a", "b", "c", "d"].map(String::from).to_vec();
    let infos = input_infos(names.to_vec(), ids);
    assert_eq!(
        infos[2],
        MidiInputInfo {
            name: "Pad (2)".into(),
            id: "c".into()
        }
    );
}

/// An input's events carry its id, and so do the note offs it sends
/// when it closes.
#[test]
fn an_input_stamps_its_events() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let EngineParts {
        mut engine,
        midi,
        mut controller,
        ..
    } = parts;
    let midi = share_midi(midi);
    let id = loom_core::MidiInputId(3);
    controller.open_midi_input(id).unwrap();
    let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
    engine.process(&mut l, &mut r);
    let mut state = MidiInputState::for_input(midi, id);
    state.receive(&[0x90, 60, 100]);
    assert!(state.holds_notes());
    engine.process(&mut l, &mut r);
    assert_eq!(engine.held_notes(id), Some(1), "the note on carried the id");
    state.release_all();
    assert!(!state.holds_notes());
    engine.process(&mut l, &mut r);
    assert_eq!(engine.held_notes(id), Some(0), "the note off carried the id");
}

/// A configuration at `rate` in `format`, as a device reports it.
fn reported(rate: u32, format: cpal::SampleFormat) -> cpal::SupportedStreamConfig {
    cpal::SupportedStreamConfig::new(2, rate, cpal::SupportedBufferSize::Unknown, format)
}

/// A range of rates in `format`, as a device reports it.
fn range(min: u32, max: u32, format: cpal::SampleFormat) -> cpal::SupportedStreamConfigRange {
    cpal::SupportedStreamConfigRange::new(2, min, max, cpal::SupportedBufferSize::Unknown, format)
}

/// Options asking for `rate` and `buffer`, on any device.
fn asking(rate: Option<u32>, buffer: Option<u32>) -> AudioOptions {
    AudioOptions {
        sample_rate: rate,
        buffer_size: buffer,
        ..AudioOptions::default()
    }
}

/// The device's float default at its own rate is used as it is, and
/// the ranges are not asked for.
#[test]
fn the_float_default_is_used_without_asking_for_ranges() {
    let config = choose_config(
        "Dev",
        reported(48_000, cpal::SampleFormat::F32),
        || -> Result<Vec<_>, HostError> { panic!("ranges were asked for") },
        &asking(None, Some(256)),
        HostError::UnsupportedFormat,
    )
    .unwrap();
    assert_eq!(config.sample_rate, 48_000);
    assert_eq!(config.buffer_size, cpal::BufferSize::Fixed(256));
}

/// Another rate, or a default that is not float, takes the first float
/// range that holds the rate.
#[test]
fn another_rate_or_format_takes_a_float_range() {
    let ranges = || {
        Ok(vec![
            range(44_100, 192_000, cpal::SampleFormat::I16),
            range(44_100, 96_000, cpal::SampleFormat::F32),
        ])
    };
    let options = asking(Some(96_000), None);
    let config = choose_config(
        "Dev",
        reported(48_000, cpal::SampleFormat::F32),
        ranges,
        &options,
        HostError::UnsupportedFormat,
    )
    .unwrap();
    assert_eq!(config.sample_rate, 96_000);
    assert_eq!(config.buffer_size, cpal::BufferSize::Default);
    let config = choose_config(
        "Dev",
        reported(48_000, cpal::SampleFormat::I16),
        ranges,
        &asking(None, None),
        HostError::UnsupportedFormat,
    )
    .unwrap();
    assert_eq!(config.sample_rate, 48_000);
}

/// With no float range at the rate, a rate asked for names the rate,
/// and no rate asked for names the format, as the caller chose.
#[test]
fn no_float_range_is_an_error() {
    let ranges = || Ok(vec![range(44_100, 48_000, cpal::SampleFormat::F32)]);
    let error = choose_config(
        "Dev",
        reported(48_000, cpal::SampleFormat::F32),
        ranges,
        &asking(Some(96_000), None),
        HostError::UnsupportedFormat,
    )
    .unwrap_err();
    assert!(
        matches!(error, HostError::UnsupportedSampleRate { rate: 96_000, .. }),
        "{error}"
    );
    let error = choose_config(
        "Dev",
        reported(48_000, cpal::SampleFormat::I16),
        || Ok(vec![range(44_100, 48_000, cpal::SampleFormat::I16)]),
        &asking(None, None),
        HostError::UnsupportedInputFormat,
    )
    .unwrap_err();
    assert!(
        matches!(error, HostError::UnsupportedInputFormat(ref name) if name == "Dev"),
        "{error}"
    );
    let error = choose_config(
        "Dev",
        reported(48_000, cpal::SampleFormat::I16),
        || -> Result<Vec<_>, HostError> { Err(HostError::Audio("gone".into())) },
        &asking(None, None),
        HostError::UnsupportedFormat,
    )
    .unwrap_err();
    assert!(matches!(error, HostError::Audio(_)), "{error}");
}

/// A held offset into Audio Out with no safety stage, so the output
/// stays at 0.5 until it fades: a thump if the stream stopped mid-way.
fn held_offset() -> EngineParts {
    held_offset_at(48_000)
}

/// The same held offset at a chosen device rate.
fn held_offset_at(rate: u32) -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(rate as f32)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let constant = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(constant, "value", 0.5).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(constant, "out", out, "in").unwrap();
    c.commit().unwrap();
    parts
}

/// A device restart after discarding its first buffer must fade in again.
#[test]
fn a_restart_after_a_dropped_buffer_starts_silent() {
    for rate in [48_000, 96_000] {
        let mut parts = held_offset_at(rate);
        let mut restart = parts.engine.restart_fade_sender();
        let stats = Arc::new(CallbackStats::default());
        let mut render = Render::new(parts.engine, 2, rate, stats, None);
        let mut first = vec![0.0; 4096 * 2];
        render.render(&mut first);
        assert_eq!(first[0], 0.0);
        assert_eq!(first[first.len() - 2], 0.5);
        // Simulate a device discarding that buffer while it pauses, then
        // accepting the first callback after the host restarts it.
        let mut resumed = vec![0.0; 4096 * 2];
        restart_stream(&mut restart, || {
            render.render(&mut resumed);
            Ok(())
        })
        .unwrap();
        assert_eq!(resumed[0], 0.0, "the first resumed sample must fade in at {rate} Hz");
        let fade_frames = rate as usize / 100;
        for (frame, samples) in resumed.as_chunks::<2>().0.iter().enumerate() {
            let expected = 0.5 * (frame.min(fade_frames) as f32 / fade_frames as f32);
            assert_eq!(*samples, [expected, expected], "frame {frame} at {rate} Hz");
        }
    }
}

/// An overload notice before the first callback is not a stream error:
/// nothing was rendered, so nothing dropped. After a callback it is, and
/// other errors always are.
#[test]
fn an_overload_before_the_first_callback_is_not_a_stream_error() {
    let stats = CallbackStats::default();
    stats.stream_error(cpal::ErrorKind::Xrun);
    stats.stream_error(cpal::ErrorKind::StreamInvalidated);
    let s = stats.snapshot();
    assert_eq!((s.start_overloads, s.stream_errors), (1, 1));

    stats.record(32, 0.1, true);
    stats.stream_error(cpal::ErrorKind::Xrun);
    let s = stats.snapshot();
    assert_eq!((s.start_overloads, s.stream_errors), (1, 2));
}

/// A short high-load callback survives until the watcher takes its peak.
#[test]
fn callback_peak_covers_every_callback_since_read() {
    let stats = CallbackStats::default();
    for _ in 0..1_000 {
        stats.record(32, 0.2, false);
    }
    stats.record(32, 0.95, false);
    for _ in 0..1_000 {
        stats.record(32, 0.2, false);
    }
    assert_eq!(stats.take_peak_since_read(), 0.95);
    assert_eq!(stats.take_peak_since_read(), 0.0);
}

#[test]
fn a_stream_stops_after_the_fade_and_two_silent_buffers() {
    let mut parts = held_offset();
    let audio = AudioOutput::clock(48_000, 64).start(parts.engine).unwrap();
    let stats = audio.stats().clone();
    std::thread::sleep(Duration::from_millis(30));
    parts.controller.fade_out().unwrap();
    let started = Instant::now();
    assert!(audio.stop(STOP_LIMIT), "stopped on the silence");
    assert!(started.elapsed() < STOP_LIMIT);
    assert!(stats.snapshot().silent_buffers >= SILENT_BUFFERS_BEFORE_STOP);
}

#[test]
fn a_stream_that_never_goes_silent_stops_at_the_limit() {
    let parts = held_offset();
    let audio = AudioOutput::clock(48_000, 64).start(parts.engine).unwrap();
    let started = Instant::now();
    // No fade-out asked for: the engine never reports silence.
    assert!(!audio.stop(STOP_LIMIT));
    let waited = started.elapsed();
    assert!(waited >= STOP_LIMIT && waited < 10 * STOP_LIMIT, "{waited:?}");
}

/// A stepped output plays only when stepped, in whole blocks, and marks
/// where each stream starts.
#[test]
fn a_stepped_output_plays_only_when_stepped() {
    let tape = Tape::new();
    let audio = AudioOutput::stepped(48_000, 64, &tape)
        .start(held_offset().engine)
        .unwrap();
    assert!(tape.is_empty());
    tape.step(100);
    assert_eq!(tape.len(), 128, "two whole blocks");
    assert_eq!(tape.left()[0], 0.0, "the fade-in starts at 0");
    drop(audio);
    tape.step(100);
    assert_eq!(tape.len(), 128, "a stopped stream plays nothing");

    let _audio = AudioOutput::stepped(48_000, 32, &tape)
        .start(held_offset().engine)
        .unwrap();
    tape.step(32);
    assert_eq!(tape.stream_starts(), vec![0, 128]);
    assert_eq!(tape.len(), 160);
}

/// Stopping a stepped stream plays its fade-out down to silence and the
/// silent buffers, counted in frames, with no sleeping.
#[test]
fn a_stepped_stop_plays_the_fade_out() {
    let tape = Tape::new();
    let mut parts = held_offset();
    let audio = AudioOutput::stepped(48_000, 64, &tape).start(parts.engine).unwrap();
    tape.step(4800);
    assert_eq!(*tape.left().last().unwrap(), 0.5);
    parts.controller.fade_out().unwrap();
    let before = tape.len();
    assert!(audio.stop(STOP_LIMIT), "stopped on the silence");
    let left = tape.left();
    assert!(left.len() > before, "the fade-out was played");
    assert!(left.len() - before <= 4800, "within the limit, in frames");
    assert_eq!(*left.last().unwrap(), 0.0, "it ends at 0");
    assert!(!tape.is_playing());
}

/// With no fade-out asked for, a stepped stop ends after the limit,
/// counted in frames.
#[test]
fn a_stepped_stop_that_never_goes_silent_stops_at_the_limit() {
    let tape = Tape::new();
    let audio = AudioOutput::stepped(48_000, 64, &tape)
        .start(held_offset().engine)
        .unwrap();
    assert!(!audio.stop(STOP_LIMIT));
    // 100 ms at 48 kHz, in whole 64-frame blocks.
    assert_eq!(tape.len(), 4800usize.div_ceil(64) * 64);
}

/// The stop limit is 100 ms for small buffers, and grows to the fade
/// plus four buffers for large ones (output safety, rule 14).
#[test]
fn the_stop_limit_grows_with_the_buffer() {
    assert_eq!(stop_limit(STOP_LIMIT, 256, 48_000), STOP_LIMIT);
    assert_eq!(stop_limit(STOP_LIMIT, 0, 48_000), STOP_LIMIT, "no buffer yet");
    let large = stop_limit(STOP_LIMIT, 4096, 44_100);
    // 10 ms + 4 * 4096 / 44100 s = 381.5 ms.
    assert!((large.as_secs_f64() - 0.3815).abs() < 0.0005, "{large:?}");
}

/// With 4096-frame blocks at 44.1 kHz, one block is 93 ms. A stop still
/// plays the whole fade and two silent blocks, where a fixed 100 ms
/// limit cut it after the fade's block, before the device played it.
#[test]
fn a_stepped_stop_with_large_blocks_plays_the_whole_fade_out() {
    let tape = Tape::new();
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(44_100.0)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let constant = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(constant, "value", 0.5).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(constant, "out", out, "in").unwrap();
    c.commit().unwrap();
    let audio = AudioOutput::stepped(44_100, 4096, &tape).start(parts.engine).unwrap();
    tape.step(4096);
    assert_eq!(*tape.left().last().unwrap(), 0.5);
    parts.controller.fade_out().unwrap();
    assert!(audio.stop(STOP_LIMIT), "stopped on the silence, not at the limit");
    let left = tape.left();
    assert!(
        left[left.len() - 2 * 4096..].iter().all(|&x| x == 0.0),
        "the last two blocks are silent"
    );
}

/// The same steps give the same tape.
#[test]
fn stepped_tapes_are_repeatable() {
    let run = || {
        let tape = Tape::new();
        let mut parts = held_offset();
        let audio = AudioOutput::stepped(44_100, 77, &tape).start(parts.engine).unwrap();
        tape.step(3000);
        parts.controller.fade_out().unwrap();
        audio.stop(STOP_LIMIT);
        tape.left()
    };
    assert_eq!(run(), run());
}

/// One recorder takes every stream at its rate, and refuses another rate.
#[test]
fn a_recorder_spans_streams_and_keeps_one_rate() {
    let dir = record::TestDir::new("host-recorder");
    let path = dir.0.join("out.wav");
    let recorder = Recorder::start(&path, 48_000).unwrap();
    let tape = Tape::new();
    for _ in 0..2 {
        let mut parts = held_offset();
        let mut output = AudioOutput::stepped(48_000, 64, &tape);
        output.record(&recorder).unwrap();
        let audio = output.start(parts.engine).unwrap();
        tape.step(1000);
        parts.controller.fade_out().unwrap();
        assert!(audio.stop(STOP_LIMIT));
    }
    let mut other = AudioOutput::stepped(44_100, 64, &tape);
    assert!(matches!(other.record(&recorder), Err(HostError::Record(_))));
    drop(recorder);

    let mut reader = hound::WavReader::open(&path).unwrap();
    let left: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).step_by(2).collect();
    assert_eq!(left, tape.left(), "the file holds both streams, nothing between");
}

#[test]
fn closing_an_input_releases_its_held_notes() {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(note, "gate", out, "in").unwrap();
    c.commit().unwrap();
    let mut state = MidiInputState::new(share_midi(parts.midi));
    let gate = |engine: &mut Engine| {
        let (mut left, mut right) = (vec![0.0; 1024], vec![0.0; 1024]);
        engine.process(&mut left, &mut right);
        left[1023]
    };
    gate(&mut parts.engine);

    state.receive(&[0x92, 60, 100]);
    state.receive(&[0x92, 62, 100]);
    state.receive(&[0x92, 62, 0]);
    assert!(state.holds_notes());
    assert_eq!(gate(&mut parts.engine), 1.0, "the note plays");

    state.release_all();
    assert!(!state.holds_notes());
    assert_eq!(gate(&mut parts.engine), 0.0, "and is released");
}

/// Held notes are kept per channel, a note on with velocity 0 lets go,
/// and other messages change nothing.
#[test]
fn held_notes_follow_note_on_and_off() {
    let mut held = HeldNotes::default();
    held.track(&[0x90, 60, 100]);
    held.track(&[0x93, 60, 100]);
    held.track(&[0x9F, 127, 1]);
    held.track(&[0x93, 62, 100]);
    held.track(&[0x93, 62, 0]);
    held.track(&[0xB0, 61, 100]);
    held.track(&[0x90, 200, 100]);
    held.track(&[0x90, 61]);
    assert!(held.any());
    assert_eq!(held.take().collect::<Vec<_>>(), [(0, 60), (3, 60), (15, 127)]);
    assert!(!held.any(), "none is held after take");
    held.track(&[0x90, 60, 100]);
    held.track(&[0x80, 60, 64]);
    assert_eq!(held, HeldNotes::default());
}

#[test]
fn sysex_and_long_messages_are_not_sent() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut state = MidiInputState::new(share_midi(parts.midi));
    state.receive(&[0xF0, 1, 2]);
    state.receive(&[0x90, 60, 100, 1]);
    state.receive(&[]);
    assert!(!state.holds_notes());
}

/// Marking a running stream dead stops it and reports `is_dead`.
#[test]
fn marking_a_running_stream_dead_reports_is_dead() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let audio = AudioOutput::clock(48_000, 64).start(parts.engine).unwrap();
    assert!(!audio.is_dead());
    assert!(!audio.stats().is_dead());
    audio.mark_dead();
    assert!(audio.is_dead());
    assert!(audio.stats().is_dead());
    assert!(audio.stats().snapshot().is_dead);
    audio.stop_immediate();
}

/// StartError implements formatting, source, and conversion to HostError.
#[test]
fn start_error_formatting_and_traits() {
    let output = AudioOutput::clock(48_000, 256);
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let err = StartError {
        output,
        engine: parts.engine,
        error: HostError::Audio("backend failed".into()),
    };
    assert_eq!(format!("{err}"), "audio error: backend failed");
    assert!(format!("{err:?}").contains("StartError"));
    use std::error::Error;
    assert!(err.source().is_some());
    let host_err: HostError = err.into();
    assert_eq!(format!("{host_err}"), "audio error: backend failed");
}

/// `AudioOutput::start` returns its error boxed, and `?` still turns it
/// into a HostError, as `loom play` does.
#[test]
fn a_boxed_start_error_converts_to_its_host_error() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let boxed = Box::new(StartError {
        output: AudioOutput::clock(48_000, 256),
        engine: parts.engine,
        error: HostError::Record("disk full".into()),
    });
    let start = || -> Result<(), HostError> { Err(boxed)? };
    assert!(matches!(start(), Err(HostError::Record(reason)) if reason == "disk full"));
}

/// CallbackStats supports clearing the dead flag and non-fatal error kinds.
#[test]
fn callback_stats_dead_flag_clearing() {
    let stats = CallbackStats::default();
    assert!(!stats.is_dead());
    stats.mark_dead();
    assert!(stats.is_dead());
    stats.clear_dead();
    assert!(!stats.is_dead());
    stats.stream_error(cpal::ErrorKind::DeviceChanged);
    assert!(!stats.is_dead());
    stats.stream_error(cpal::ErrorKind::StreamInvalidated);
    assert!(stats.is_dead());
}

/// StreamHandle drops its stream and safely frees the Render pointer.
#[test]
fn stream_handle_drops_safely() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let stats = Arc::new(CallbackStats::default());
    let render = Render::new(parts.engine, 2, 48_000, stats, None);
    let render_ptr = Box::into_raw(Box::new(render));
    let handle = StreamHandle {
        stream: None,
        render: render_ptr,
    };
    drop(handle);
}

/// RenderPtr safely delegates render calls.
#[test]
fn render_ptr_delegates_render() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let stats = Arc::new(CallbackStats::default());
    let render = Render::new(parts.engine, 2, 48_000, stats, None);
    let render_ptr = Box::into_raw(Box::new(render));
    let ptr = RenderPtr(render_ptr);
    let mut buffer = [0.0f32; 64];
    // SAFETY: render_ptr is uniquely owned in this test and freed after render completes.
    unsafe {
        ptr.render(&mut buffer);
        drop(Box::from_raw(render_ptr));
    }
}

/// Render::into_parts returns engine and optional tap.
#[test]
fn render_into_parts_returns_engine_and_tap() {
    let parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let stats = Arc::new(CallbackStats::default());
    let render = Render::new(parts.engine, 2, 48_000, stats, None);
    let (engine, tap) = render.into_parts();
    assert!(tap.is_none());
    assert!(!engine.is_silent());
}
