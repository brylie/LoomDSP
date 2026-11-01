use super::*;

/// A removed or replaced system input is reported even without a callback error.
#[test]
fn watcher_reports_a_missing_or_changed_input_device() {
    use iced::futures::StreamExt;
    use std::sync::atomic::AtomicUsize;

    static LIST_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn missing() -> Result<crate::devices::Inputs, String> {
        LIST_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(crate::devices::Inputs {
            default: Some("Other".into()),
            devices: vec!["Other".into()],
        })
    }
    fn changed_default() -> Result<crate::devices::Inputs, String> {
        LIST_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(crate::devices::Inputs {
            default: Some("Other".into()),
            devices: vec!["Test input".into(), "Other".into()],
        })
    }
    fn unchanged() -> Result<crate::devices::Inputs, String> {
        LIST_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(crate::devices::Inputs {
            default: Some("Test input".into()),
            devices: vec!["Test input".into(), "Other".into()],
        })
    }
    for (list, system_default, should_fail) in [
        (missing as fn() -> _, false, true),
        (changed_default, true, true),
        (unchanged, true, false),
    ] {
        LIST_CALLS.store(0, Ordering::Relaxed);
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
        let watch = Watch {
            status: writer.status(),
            stats: parts.controller.stats().clone(),
            tracker: Arc::new(Tracker::new((0, 0))),
            device: "Test input".into(),
            system_default,
            list: Some(list),
            permission_denied: None,
            active: Vec::new(),
            displays: parts.controller.display_reader(),
        };
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut events = Box::pin(stream(&watch));
            iced::futures::executor::block_on(async {
                while let Some(event) = events.next().await {
                    if event.failed {
                        let _ = send.send(());
                        break;
                    }
                }
            });
        });
        let status = writer.status();
        let deadline = Instant::now() + Duration::from_secs(if should_fail { 5 } else { 2 });
        let mut failed = false;
        while Instant::now() < deadline {
            // Keep callbacks flowing so only the device list can fail the stream.
            writer.callback_received();
            if receive.recv_timeout(Duration::from_millis(50)).is_ok() {
                failed = true;
                break;
            }
        }
        let remained_live = status.is_live();
        if !failed {
            // Stop the watcher cleanly after the control case or a timeout.
            status.fail();
            let _ = receive.recv_timeout(Duration::from_secs(2));
        }
        worker.join().unwrap();
        assert!(LIST_CALLS.load(Ordering::Relaxed) > 0, "the device list was checked");
        assert_eq!(failed, should_fail, "the list determines stream failure");
        assert_eq!(remained_live, !should_fail);
    }
}

/// The watcher detects permission denied after a stream started.
#[test]
fn watcher_reports_a_late_permission_denial() {
    use iced::futures::StreamExt;
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
    let watch = Watch {
        status: writer.status(),
        stats: parts.controller.stats().clone(),
        tracker: Arc::new(Tracker::new((0, 0))),
        device: "Test input".into(),
        system_default: false,
        list: None,
        permission_denied: Some(|| true),
        active: Vec::new(),
        displays: parts.controller.display_reader(),
    };
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut events = Box::pin(stream(&watch));
        iced::futures::executor::block_on(async {
            while let Some(event) = events.next().await {
                if event.failed {
                    let _ = send.send(());
                    break;
                }
            }
        });
    });
    receive
        .recv_timeout(Duration::from_secs(4))
        .expect("permission denial was reported");
    assert!(!writer.status().is_live());
}

/// A replacement watcher keeps the original warning deadline.
#[test]
fn drift_warning_survives_a_watcher_restart() {
    let start = Instant::now();
    let tracker = Arc::new(Tracker::new((0, 0)));
    assert!(tracker.read((1, 0), false, false, start).show);
    let replacement = tracker.clone();
    assert!(
        replacement
            .read((1, 0), false, false, start + Duration::from_secs(9))
            .show
    );
    assert!(
        !replacement
            .read((1, 0), false, false, start + Duration::from_secs(10))
            .show
    );
    assert!(
        replacement
            .read((1, 1), false, false, start + Duration::from_secs(11))
            .show
    );
}

/// A restarted stream sends its current warning state on its first read.
#[test]
fn restarted_stream_reports_the_current_warning() {
    use iced::futures::StreamExt;
    for (age, shown) in [(2, true), (12, false)] {
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
        let tracker = Arc::new(Tracker::new((0, 0)));
        tracker.read((1, 0), false, false, Instant::now() - Duration::from_secs(age));
        parts.controller.stats().input_underruns.store(1, Ordering::Relaxed);
        let watch = Watch {
            status: writer.status(),
            stats: parts.controller.stats().clone(),
            tracker,
            device: "Test input".into(),
            system_default: false,
            list: None,
            permission_denied: None,
            active: Vec::new(),
            displays: parts.controller.display_reader(),
        };
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut events = Box::pin(stream(&watch));
            let event = iced::futures::executor::block_on(events.next());
            let _ = send.send(event);
        });
        let event = receive
            .recv_timeout(Duration::from_secs(3))
            .expect("watcher sends its first state")
            .unwrap();
        assert_eq!((event.underruns, event.overruns, event.show), (1, 0, shown));
    }
}

/// An older read from a retiring watcher cannot roll counters back.
#[test]
fn stale_watcher_reads_do_not_restart_the_warning() {
    let start = Instant::now();
    let tracker = Tracker::new((0, 0));
    assert!(tracker.read((2, 1), false, false, start).show);
    let stale = tracker.read((1, 1), false, false, start + Duration::from_secs(1));
    assert_eq!((stale.underruns, stale.overruns), (2, 1));
    assert!(!tracker.read((2, 1), false, false, start + Duration::from_secs(10)).show);
}

/// A correction is shown for ten seconds; a failed stream is reported.
#[test]
fn correction_warning_expires_without_repeated_updates() {
    let start = Instant::now();
    let mut state = State::new((0, 0));
    assert!(!state.read((0, 0), false, false, start).show);
    assert!(state.read((1, 0), false, true, start).show);
    assert!(state.read((1, 0), false, true, start + Duration::from_secs(9)).show);
    let expired = state.read((1, 0), true, false, start + Duration::from_secs(10));
    assert!(!expired.show);
    assert!(expired.failed);
}
