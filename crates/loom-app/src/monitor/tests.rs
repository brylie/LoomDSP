use super::*;

/// A running engine's meter reaches the subscription without an app
/// timer, and the update names only the watched probe.
#[test]
fn telemetry_stream_reports_a_watched_meter() {
    use iced::futures::StreamExt;
    use loom_core::{EngineConfig, ModuleId, ProbeKind, create_engine};
    use loom_patch::Patch;
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches/sine.ron");
    Patch::load(path).unwrap().load_into(&mut parts.controller).unwrap();
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
