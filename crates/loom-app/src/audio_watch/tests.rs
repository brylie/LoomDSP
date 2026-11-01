use super::*;

/// A stopped callback stream wakes the app without a UI timer, and a
/// new stream gets a distinct subscription identity.
#[test]
fn stream_reports_a_stopped_device() {
    use iced::futures::StreamExt;
    use std::collections::hash_map::DefaultHasher;
    let watch = Watch {
        stats: Arc::new(CallbackStats::default()),
        engine: Arc::new(EngineStats::default()),
    };
    let hash = |watch: &Watch| {
        let mut hasher = DefaultHasher::new();
        watch.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(hash(&watch), hash(&watch.clone()));
    assert_ne!(
        hash(&watch),
        hash(&Watch {
            stats: Arc::new(CallbackStats::default()),
            engine: watch.engine.clone(),
        })
    );
    let mut updates = stream(&watch);
    let first = iced::futures::executor::block_on(updates.next());
    assert!(matches!(first, Some(Event::Stalled { callbacks: 0, .. })));
    let second = iced::futures::executor::block_on(updates.next());
    assert!(matches!(second, Some(Event::Stalled { callbacks: 0, .. })));
}

fn stats(callbacks: u64, load: f64, overruns: u64) -> StatsSnapshot {
    StatsSnapshot {
        callbacks,
        last_load: load,
        max_load: load,
        overruns,
        ..StatsSnapshot::default()
    }
}

/// Steady callbacks at a steady load: nothing to send, so an idle
/// patch draws no frames (#90).
#[test]
fn steady_audio_sends_nothing_after_first_load_tick() {
    let start = Instant::now();
    let mut watcher = Watcher::new(start);
    for read in 1..=40u64 {
        let now = start + INTERVAL * read as u32;
        let events = watcher.read(&stats(read * 10, 0.02, 0), 0, now);
        assert_eq!(events.len(), usize::from(read == 2), "read {read}");
    }
}

/// A load change of one percent or more shows within two reads, once.
/// Less than half a percent shows the same text, so nothing is sent.
#[test]
fn a_load_change_is_sent_once() {
    let start = Instant::now();
    let mut watcher = Watcher::new(start);
    let mut sent = Vec::new();
    for read in 0..10u32 {
        let load = match read {
            0..2 => 0.02,
            2..4 => 0.021,
            _ => 0.05,
        };
        let now = start + INTERVAL * read;
        sent.extend(watcher.read(&stats(u64::from(read) + 1, load, 0), 0, now));
    }
    assert_eq!(
        sent,
        vec![
            Event::Stats {
                dropouts: 0,
                at: start + INTERVAL,
                peak: 0.02,
                max: 0.02
            },
            Event::Stats {
                dropouts: 0,
                at: start + INTERVAL * 5,
                peak: 0.05,
                max: 0.05
            },
        ]
    );
}

/// One callback spike colours its tick and leaves the ten-second max.
#[test]
fn callback_spike_is_visible_then_expires() {
    let start = Instant::now();
    let mut watcher = Watcher::new(start);
    let low = stats(1, 0.2, 0);
    let _ = watcher.read_peak(&low, 0.2, 0, start);
    let spike = watcher.read_peak(&low, 0.95, 0, start + INTERVAL);
    assert!(spike.iter().any(|event| matches!(
        event,
        Event::Stats {
            peak: 0.95,
            max: 0.95,
            ..
        }
    )));
    let mut expired = false;
    for read in 2..44 {
        let events = watcher.read_peak(&low, 0.2, 0, start + INTERVAL * read);
        expired |= events
            .iter()
            .any(|event| matches!(event, Event::Stats { max: 0.2, .. }));
    }
    assert!(expired, "the max should fall after ten seconds of quiet");
}

/// A dropout is sent, and so is the end of its alarm ten seconds later.
/// Nothing is sent in between.
#[test]
fn dropouts_and_the_end_of_their_alarm_are_sent() {
    let start = Instant::now();
    let mut watcher = Watcher::new(start);
    let mut sent = Vec::new();
    for read in 0..60u32 {
        let now = start + INTERVAL * read;
        let overruns = u64::from(read >= 4);
        for event in watcher.read(&stats(u64::from(read) + 1, 0.02, overruns), 0, now) {
            sent.push((read, event));
        }
    }
    let at = |read: u32| start + INTERVAL * read;
    assert_eq!(
        sent,
        vec![
            (
                1,
                Event::Stats {
                    dropouts: 0,
                    at: at(1),
                    peak: 0.02,
                    max: 0.02
                }
            ),
            (
                4,
                Event::Stats {
                    dropouts: 1,
                    at: at(4),
                    peak: 0.02,
                    max: 0.02
                }
            ),
            // Red for ten seconds from read 4: off at read 44.
            (
                44,
                Event::Stats {
                    dropouts: 1,
                    at: at(44),
                    peak: 0.02,
                    max: 0.02,
                }
            ),
        ]
    );
}

/// Callbacks that stop advancing are reported on every read, with the
/// time they last advanced, so the UI times the stall from there.
#[test]
fn a_stall_is_reported_with_the_last_advance() {
    let start = Instant::now();
    let mut watcher = Watcher::new(start);
    assert!(watcher.read(&stats(5, 0.02, 0), 0, start).is_empty());
    let later = start + INTERVAL;
    assert_eq!(
        watcher.read(&stats(5, 0.02, 0), 0, later),
        vec![
            Event::Stalled {
                callbacks: 5,
                advanced: start
            },
            Event::Stats {
                dropouts: 0,
                at: later,
                peak: 0.02,
                max: 0.02
            },
        ]
    );
    let dead = StatsSnapshot {
        is_dead: true,
        ..stats(9, 0.02, 0)
    };
    assert!(matches!(
        watcher.read(&dead, 0, later + INTERVAL)[..],
        [Event::Stalled { callbacks: 9, .. }, ..]
    ));
}
