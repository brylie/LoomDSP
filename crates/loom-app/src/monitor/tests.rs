use super::*;

/// A running engine's meter reaches the subscription without an app
/// timer, and the update names only the watched probe.
#[test]
fn telemetry_stream_reports_a_watched_meter() {
    use iced::futures::StreamExt;
    use loom_core::{EngineConfig, ModuleId, ProbeKind, create_engine};
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches/sine.ron");
    crate::patch_file::load(path)
        .unwrap()
        .load_into(&mut parts.controller)
        .unwrap();
    let output = parts.controller.output_ref(ModuleId(3), "out").unwrap();
    let probe = parts.controller.add_probe(output, ProbeKind::Meter).unwrap();
    parts.controller.commit().unwrap();
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    for _ in 0..10 {
        parts.engine.process(&mut left, &mut right);
    }
    let monitor = Monitor::new(parts.telemetry, 48_000.0, 1);
    monitor.watch(WatchList {
        meters: HashSet::from([probe]),
        ..WatchList::default()
    });
    let mut updates = stream(&monitor);
    let update = iced::futures::executor::block_on(updates.next()).unwrap();
    assert_eq!(update.levels.len(), 1);
    assert_eq!(update.levels[0].0, probe);
    assert!(update.levels[0].1.peak > 0.0);
}

#[test]
fn only_visible_changes_are_sent() {
    let probe = ProbeId(1);
    let mut shown = HashMap::new();
    let silent = [(probe, Level::default())];
    // A new probe at level 0 looks the same as no meter: nothing to send.
    assert!(changed(&shown, &silent).is_empty());
    let loud = [(
        probe,
        Level {
            rms: 0.5,
            peak: 0.8,
            right: None,
        },
    )];
    assert_eq!(changed(&shown, &loud).len(), 1);
    shown.insert(probe, steps(loud[0].1));
    // Less than one step of 256: not drawn, not sent.
    let close = [(
        probe,
        Level {
            rms: 0.501,
            peak: 0.799,
            right: None,
        },
    )];
    assert!(changed(&shown, &close).is_empty());
}

/// A change on the right side wakes the UI even when the left is still.
#[test]
fn right_meter_changes_are_visible() {
    let probe = ProbeId(1);
    let now = Instant::now();
    let mut levels = Levels::default();
    let reading = MeterReading {
        probe,
        frame: 0,
        frames: 64,
        peak: 0.0,
        rms: 0.0,
        right: Some((0.5, 0.5)),
    };
    levels.update(&[reading], now);
    let level = levels.get(probe).unwrap();
    assert_eq!(level.peak, 0.0);
    assert!(level.right.unwrap().1 > 0.0);
    assert_eq!(changed(&HashMap::new(), &[(probe, level)]).len(), 1);
}

fn chunk(probe: ProbeId, frame: u64, value: impl Fn(u64) -> f32) -> ScopeChunk {
    let mut samples = [0.0; loom_core::SCOPE_CHUNK];
    for (i, s) in samples.iter_mut().enumerate() {
        *s = value(frame + i as u64);
    }
    ScopeChunk {
        probe,
        frame,
        len: loom_core::SCOPE_CHUNK as u32,
        samples,
    }
}

fn scope_watch(probe: ProbeId, full: bool) -> WatchList {
    WatchList {
        scopes: HashMap::from([(
            probe,
            ScopeWant {
                window: 480,
                trigger: Trigger::Auto,
                slope: Slope::Rising,
            },
        )]),
        full_traces: full,
        ..WatchList::default()
    }
}

/// Feeds the watcher 16 ms of a signal per call, as the thread does at
/// 60 fps with 48 kHz audio, and returns how many updates carried a
/// trace.
fn traces_sent(watch: &WatchList, value: impl Fn(u64) -> f32 + Copy, calls: u64) -> usize {
    let probe = *watch.scopes.keys().next().unwrap();
    let mut watcher = Watcher::default();
    let start = Instant::now();
    let mut sent = 0;
    let chunk_frames = loom_core::SCOPE_CHUNK as u64;
    let per_call = 12; // 768 frames, 16 ms
    for call in 0..calls {
        let chunks: Vec<ScopeChunk> = (0..per_call)
            .map(|i| chunk(probe, (call * per_call + i) * chunk_frames, value))
            .collect();
        let update = watcher.take(watch, &[], chunks, 48_000.0, start + SCOPE_INTERVAL * call as u32);
        if !update.traces.is_empty() {
            sent += 1;
        }
        watcher.sent(&update);
    }
    sent
}

/// A silent or constant scope sends one trace, then none: nothing on
/// screen changes (panels.md, Drawing the scope trace, Redraw).
#[test]
fn a_still_signal_sends_no_traces() {
    let watch = scope_watch(ProbeId(3), true);
    assert_eq!(traces_sent(&watch, |_| 0.0, 30), 1);
    assert_eq!(traces_sent(&watch, |_| 0.5, 30), 1);
}

/// A moving signal sends a trace each time it changes. At the middle
/// zoom only the level is drawn, so a steady wave sends one.
#[test]
fn a_moving_signal_sends_traces_close_up_and_only_its_level_further_out() {
    let saw = |frame: u64| (std::f32::consts::TAU * 440.0 * frame as f32 / 48_000.0).sin();
    assert!(traces_sent(&scope_watch(ProbeId(3), true), saw, 30) > 20);
    assert_eq!(traces_sent(&scope_watch(ProbeId(3), false), saw, 30), 1);
}

/// panels.md, Signal activity on ports: carrying within one read of a
/// signal above -80 dB, a one-sample trigger counts, and silent 400 ms
/// after it stops. Each change is sent once.
#[test]
fn activity_turns_on_at_once_and_off_after_two_quiet_reads() {
    let probe = ProbeId(4);
    let watch = WatchList {
        activity: HashSet::from([probe]),
        ..WatchList::default()
    };
    let mut watcher = Watcher::default();
    let start = Instant::now();
    let mut changes = Vec::new();
    // The thread drains every 33 ms. One reading at 99 ms holds a
    // one-sample trigger: its block's peak is 1.0. Then silence.
    for tick in 0..30u32 {
        let ms = tick * 33;
        let peak = if ms == 99 { 1.0 } else { 0.0 };
        let reading = MeterReading {
            probe,
            frame: 0,
            frames: 128,
            peak,
            rms: peak / 11.3,
            right: None,
        };
        let now = start + Duration::from_millis(u64::from(ms));
        let update = watcher.take(&watch, &[reading], [], 48_000.0, now);
        for &(_, carrying) in &update.activity {
            changes.push((ms, carrying));
        }
        watcher.sent(&update);
    }
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert_eq!(changes[0], (99, true), "carrying at once");
    let off = changes[1];
    assert!(!off.1);
    assert!(
        (99 + 300..=99 + 500).contains(&off.0),
        "silent about 400 ms later: {changes:?}"
    );
    // Below -80 dB is silent.
    let mut watcher = Watcher::default();
    let quiet = MeterReading {
        probe,
        frame: 0,
        frames: 128,
        peak: 0.000_05,
        rms: 0.0,
        right: None,
    };
    assert!(watcher.take(&watch, &[quiet], [], 48_000.0, start).is_empty());
}

/// Readings of probes the UI does not draw as meters are not sent: an
/// activity probe must not redraw the window.
#[test]
fn only_drawn_meters_are_sent() {
    let mut watcher = Watcher::default();
    let reading = |probe| MeterReading {
        probe: ProbeId(probe),
        frame: 0,
        frames: 128,
        peak: 0.5,
        rms: 0.3,
        right: None,
    };
    let watch = WatchList {
        meters: HashSet::from([ProbeId(1)]),
        ..WatchList::default()
    };
    let update = watcher.take(&watch, &[reading(1), reading(2)], [], 48_000.0, Instant::now());
    assert_eq!(update.levels.len(), 1);
    assert_eq!(update.levels[0].0, ProbeId(1));
    watcher.sent(&update);
    let again = watcher.take(&watch, &[reading(1), reading(2)], [], 48_000.0, Instant::now());
    assert!(again.is_empty(), "{again:?}");
}

/// One block's reading of a level, for a history.
fn reading(probe: ProbeId, frame: u64, rms: f32) -> MeterReading {
    MeterReading {
        probe,
        frame,
        frames: 128,
        peak: rms,
        rms,
        right: None,
    }
}

/// Runs a watcher over `seconds` of readings, in steps like the thread's, and
/// counts the updates that carry a history.
fn history_updates(watcher: &mut Watcher, watch: &WatchList, seconds: f32, level: impl Fn(u64) -> f32) -> usize {
    let probe = *watch.histories.keys().next().unwrap();
    let mut sends = 0;
    let now = Instant::now();
    let steps = (seconds * 48_000.0 / 1280.0) as u64;
    for step in 0..steps {
        let readings: Vec<MeterReading> = (0..10)
            .map(|i| {
                let frame = (step * 10 + i) * 128;
                reading(probe, frame, level(frame))
            })
            .collect();
        let update = watcher.take(watch, &readings, Vec::new(), 48_000.0, now);
        if !update.histories.is_empty() {
            sends += 1;
        }
        watcher.sent(&update);
    }
    sends
}

/// A list watching one level history of one second.
fn history_watch() -> WatchList {
    WatchList {
        histories: HashMap::from([(ProbeId(1), 1.0)]),
        ..WatchList::default()
    }
}

/// A silent input sends the UI nothing (panel-widgets.md, rule 7).
#[test]
fn a_silent_history_sends_nothing() {
    let mut watcher = Watcher::default();
    assert_eq!(history_updates(&mut watcher, &history_watch(), 2.0, |_| 0.0), 0);
}

/// A steady tone stops sending once its columns have filled the window, even
/// with rounding noise on each reading.
#[test]
fn a_steady_history_stops_sending_once_settled() {
    let mut watcher = Watcher::default();
    let watch = history_watch();
    let noisy = |frame: u64| 0.5 + 1e-4 * (frame as f32).sin();
    assert!(history_updates(&mut watcher, &watch, 2.0, noisy) > 0, "filling");
    assert_eq!(history_updates(&mut watcher, &watch, 2.0, noisy), 0, "settled");
}

/// Changing the window starts the history again, on that window.
#[test]
fn a_new_window_starts_the_history_again() {
    let mut watcher = Watcher::default();
    let mut watch = history_watch();
    history_updates(&mut watcher, &watch, 1.0, |_| 0.5);
    watch.histories.insert(ProbeId(1), 5.0);
    watcher.take(&watch, &[], Vec::new(), 48_000.0, Instant::now());
    assert_eq!(watcher.histories[&ProbeId(1)].window(), 5.0);
}

/// [`moved`] ignores what is inside the tolerance, and counts a change of
/// length, or a trace where nothing was shown.
#[test]
fn moved_reads_a_tolerance() {
    let trace = |y: f32| loom_canvas::Trace {
        points: vec![[0.0, y]],
        ..loom_canvas::Trace::default()
    };
    assert!(!moved(None, &loom_canvas::Trace::default(), 0.1));
    assert!(moved(None, &trace(0.0), 0.1));
    assert!(!moved(Some(&trace(0.0)), &trace(0.05), 0.1));
    assert!(moved(Some(&trace(0.0)), &trace(0.2), 0.1));
}

/// One chunk of a sine at engine frame `frame`.
fn sine_chunk(probe: ProbeId, frame: u64, amplitude: f32) -> ScopeChunk {
    let mut chunk = ScopeChunk {
        probe,
        frame,
        len: loom_core::SCOPE_CHUNK as u32,
        samples: [0.0; loom_core::SCOPE_CHUNK],
    };
    for (i, sample) in chunk.samples.iter_mut().enumerate() {
        let at = (frame + i as u64) as f64 / 48_000.0;
        *sample = amplitude * (std::f64::consts::TAU * 1000.0 * at).sin() as f32;
    }
    chunk
}

/// Runs a watcher over `steps` steps of chunks, and counts the updates that
/// carry a spectrum.
fn spectrum_updates(watcher: &mut Watcher, watch: &WatchList, start: &mut u64, steps: u64, amplitude: f32) -> usize {
    let probe = *watch.spectra.keys().next().unwrap();
    let mut sends = 0;
    for _ in 0..steps {
        let chunks: Vec<ScopeChunk> = (0..12)
            .map(|i| sine_chunk(probe, *start + i * loom_core::SCOPE_CHUNK as u64, amplitude))
            .collect();
        *start += 12 * loom_core::SCOPE_CHUNK as u64;
        let update = watcher.take(watch, &[], chunks, 48_000.0, Instant::now());
        if !update.spectra.is_empty() {
            sends += 1;
        }
        watcher.sent(&update);
    }
    sends
}

/// A list watching one spectrum.
fn spectrum_watch(hold: bool) -> WatchList {
    WatchList {
        spectra: HashMap::from([(ProbeId(1), SpectrumWant { tilt: false, hold })]),
        ..WatchList::default()
    }
}

/// A silent input sends the UI nothing (panel-widgets.md, rule 7).
#[test]
fn a_silent_spectrum_sends_nothing() {
    let mut watcher = Watcher::default();
    let mut frame = 0;
    assert_eq!(
        spectrum_updates(&mut watcher, &spectrum_watch(false), &mut frame, 100, 0.0),
        0
    );
}

/// A steady tone stops sending once the line has settled, with or without the
/// hold line (panel-widgets.md, Spectrum, Data and redraw).
#[test]
fn a_steady_spectrum_stops_sending_once_settled() {
    for hold in [false, true] {
        let mut watcher = Watcher::default();
        let watch = spectrum_watch(hold);
        let mut frame = 0;
        assert!(
            spectrum_updates(&mut watcher, &watch, &mut frame, 100, 0.5) > 0,
            "starting"
        );
        assert_eq!(
            spectrum_updates(&mut watcher, &watch, &mut frame, 100, 0.5),
            0,
            "settled"
        );
    }
}

/// A scope and a spectrum on one output share a probe, and each gets its own
/// result.
#[test]
fn a_scope_and_a_spectrum_on_one_probe_stay_apart() {
    let probe = ProbeId(1);
    let mut watcher = Watcher::default();
    let mut watch = spectrum_watch(false);
    watch.scopes.insert(
        probe,
        ScopeWant {
            window: 480,
            trigger: Trigger::Auto,
            slope: Slope::Rising,
        },
    );
    watch.full_traces = true;
    let chunks: Vec<ScopeChunk> = (0..100).map(|i| sine_chunk(probe, i * 64, 0.5)).collect();
    let update = watcher.take(&watch, &[], chunks, 48_000.0, Instant::now());
    assert_eq!(update.traces.len(), 1);
    assert_eq!(update.spectra.len(), 1);
    assert_ne!(update.traces[0].1, update.spectra[0].1);
    assert_eq!(update.spectra[0].1.points.len(), crate::spectrum::COLUMNS);
}

/// One chunk of pairs for an XY scope.
fn pair_chunk(probe: ProbeId, frame: u64, pair: impl Fn(u64) -> [f32; 2]) -> loom_core::PairChunk {
    let mut chunk = loom_core::PairChunk {
        probe,
        frame,
        len: loom_core::PAIR_CHUNK as u32,
        values: [0.0; loom_core::PAIR_CHUNK * 2],
    };
    for (i, out) in chunk.values.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        out.copy_from_slice(&pair(frame + i as u64));
    }
    chunk
}

/// A list watching one XY scope.
fn xy_watch() -> WatchList {
    WatchList {
        xy: HashMap::from([(
            ProbeId(1),
            XyWant {
                frames: 480,
                range: 1.0,
                rotate: false,
            },
        )]),
        full_traces: true,
        ..WatchList::default()
    }
}

/// Runs a watcher over `steps` steps of pair chunks, and counts the sends.
fn xy_sends(
    watcher: &mut Watcher,
    watch: &WatchList,
    start: &mut u64,
    steps: u64,
    pair: impl Fn(u64) -> [f32; 2],
) -> usize {
    let mut sends = 0;
    for _ in 0..steps {
        let chunk = pair_chunk(ProbeId(1), *start, &pair);
        *start += loom_core::PAIR_CHUNK as u64;
        let xy = watcher.take_pairs(watch, [chunk], 48_000.0);
        if !xy.is_empty() {
            sends += 1;
        }
        watcher.sent(&Update {
            xy,
            ..Update::default()
        });
    }
    sends
}

/// A silent XY scope sends nothing, and a still one sends once, so neither
/// draws frames (panel-widgets.md, rule 7). A moving one keeps sending.
#[test]
fn only_a_moving_xy_scope_sends() {
    let watch = xy_watch();
    let mut frame = 0;
    assert_eq!(
        xy_sends(&mut Watcher::default(), &watch, &mut frame, 50, |_| [0.0, 0.0]),
        0,
        "silent"
    );
    let mut still = Watcher::default();
    let mut frame = 0;
    xy_sends(&mut still, &watch, &mut frame, 20, |_| [0.25, 0.5]);
    assert_eq!(
        xy_sends(&mut still, &watch, &mut frame, 20, |_| [0.25, 0.5]),
        0,
        "still"
    );
    let mut moving = Watcher::default();
    let mut frame = 0;
    let wave = |f: u64| [(f as f32 * 0.03).sin() * 0.5, (f as f32 * 0.02).cos() * 0.5];
    assert_eq!(xy_sends(&mut moving, &watch, &mut frame, 20, wave), 20, "moving");
}

/// A trace leaves with its scope.
#[test]
fn an_xy_scope_off_screen_keeps_nothing() {
    let mut watcher = Watcher::default();
    let mut frame = 0;
    xy_sends(&mut watcher, &xy_watch(), &mut frame, 5, |_| [0.25, 0.5]);
    assert!(watcher.take_pairs(&WatchList::default(), [], 48_000.0).is_empty());
    assert!(watcher.xy.is_empty() && watcher.shown_xy.is_empty());
}
