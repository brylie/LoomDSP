use super::*;

/// A watch on a stream with no modules, for the tests of the real worker.
fn watch_of(writer: &loom_core::audio_input::InputWriter, parts: &loom_core::EngineParts) -> Watch {
    Watch {
        status: writer.status(),
        stats: parts.controller.stats().clone(),
        tracker: Arc::new(Tracker::new((0, 0))),
        device: "Test input".into(),
        system_default: false,
        list: None,
        permission_denied: None,
        active: Vec::new(),
        displays: parts.controller.display_reader(),
    }
}

/// The inputs of a machine: the first is the system default.
fn inputs(names: &[&str]) -> Result<crate::devices::Inputs, String> {
    Ok(crate::devices::Inputs {
        default: names.first().map(|name| (*name).to_string()),
        devices: names.iter().map(|name| (*name).to_string()).collect(),
    })
}

/// Seconds after a start time.
fn after(start: Instant, seconds: f64) -> Instant {
    start + Duration::from_secs_f64(seconds)
}

/// A new stream may stay quiet for ten seconds while the device starts.
#[test]
fn a_stream_that_has_not_started_gets_ten_seconds() {
    let start = Instant::now();
    let mut health = Health::new(0, start, true);
    assert!(!health.tick(0, after(start, 10.0)).stalled);
    assert!(health.tick(0, after(start, 10.25)).stalled);
}

/// After its first callback, a stream may stay quiet for one second.
#[test]
fn a_running_stream_stalls_after_one_second() {
    let start = Instant::now();
    let mut health = Health::new(0, start, true);
    assert!(!health.tick(1, after(start, 5.0)).stalled);
    assert!(!health.tick(1, after(start, 6.0)).stalled);
    assert!(health.tick(1, after(start, 6.25)).stalled);
}

/// A stream that has already called back never gets the long start limit.
#[test]
fn a_stream_opened_with_callbacks_gets_the_short_limit() {
    let start = Instant::now();
    let mut health = Health::new(7, start, true);
    assert!(health.tick(7, after(start, 1.25)).stalled);
}

/// Callbacks that keep arriving never stall, however long the stream runs.
#[test]
fn callback_progress_keeps_a_stream_live() {
    let start = Instant::now();
    let mut health = Health::new(0, start, true);
    for step in 1..=200_u32 {
        let tick = health.tick(u64::from(step), after(start, f64::from(step) * 0.25));
        assert!(!tick.stalled, "step {step}");
    }
}

/// A later stall is found after a long healthy run, and recovery clears it.
#[test]
fn a_later_stall_is_found_and_progress_clears_it() {
    let start = Instant::now();
    let mut health = Health::new(0, start, true);
    assert!(!health.tick(5, after(start, 100.0)).stalled);
    assert!(health.tick(5, after(start, 101.25)).stalled);
    assert!(!health.tick(6, after(start, 101.5)).stalled);
}

/// Without a device list there is no real stream, so silence is not a failure.
#[test]
fn hosts_that_cannot_list_never_stall() {
    let start = Instant::now();
    let mut health = Health::new(0, start, false);
    assert!(!health.tick(0, after(start, 60.0)).stalled);
}

/// The permission and the list are read once a second, not on every look.
#[test]
fn checks_run_once_a_second() {
    let start = Instant::now();
    let mut health = Health::new(0, start, true);
    assert!(!health.tick(1, after(start, 0.25)).check_due);
    assert!(!health.tick(2, after(start, 0.75)).check_due);
    assert!(health.tick(3, after(start, 1.0)).check_due);
    assert!(!health.tick(4, after(start, 1.25)).check_due);
    assert!(health.tick(5, after(start, 2.0)).check_due);
}

/// A named input is lost when it leaves the list, and a new default does not
/// matter to it.
#[test]
fn a_named_input_fails_only_when_it_is_missing() {
    let gone = inputs(&["Other"]);
    assert_eq!(
        check_failure(false, Some(&gone), "Mic", false),
        Some(Failure::DeviceMissing)
    );
    let not_default = inputs(&["Other", "Mic"]);
    assert_eq!(check_failure(false, Some(&not_default), "Mic", false), None);
}

/// A stream on the system default fails when the default moves, for a reason
/// that a missing device does not share.
#[test]
fn a_default_input_fails_when_the_default_changes() {
    let moved = inputs(&["Other", "Mic"]);
    assert_eq!(
        check_failure(false, Some(&moved), "Mic", true),
        Some(Failure::DefaultChanged)
    );
    let gone = inputs(&["Other"]);
    assert_eq!(
        check_failure(false, Some(&gone), "Mic", true),
        Some(Failure::DeviceMissing)
    );
}

/// A stable device stays live, and a stream's advancing callbacks do not
/// hide a list that says the device is gone.
#[test]
fn a_stable_device_stays_live_and_progress_does_not_hide_a_loss() {
    let start = Instant::now();
    let mut health = Health::new(0, start, true);
    let stable = inputs(&["Mic", "Other"]);
    let gone = inputs(&["Other"]);
    for step in 1..=8_u32 {
        let tick = health.tick(u64::from(step), after(start, f64::from(step) * 0.25));
        assert!(!tick.stalled);
        assert_eq!(check_failure(false, Some(&stable), "Mic", true), None);
    }
    let tick = health.tick(9, after(start, 2.25));
    assert!(!tick.stalled, "only the list reports this loss");
    assert_eq!(
        check_failure(false, Some(&gone), "Mic", true),
        Some(Failure::DeviceMissing)
    );
}

/// A list that cannot be read for a moment does not interrupt a live input,
/// and neither does a host with no list.
#[test]
fn a_transient_list_error_changes_nothing() {
    let error = Err("the list is busy".to_string());
    assert_eq!(check_failure(false, Some(&error), "Mic", true), None);
    assert_eq!(check_failure(false, None, "Mic", true), None);
}

/// A refused permission fails the stream, whatever the list says.
#[test]
fn permission_denial_fails_the_stream() {
    let stable = inputs(&["Mic"]);
    assert_eq!(
        check_failure(true, Some(&stable), "Mic", true),
        Some(Failure::PermissionDenied)
    );
    assert_eq!(check_failure(true, None, "Mic", true), Some(Failure::PermissionDenied));
    let error = Err("the list is busy".to_string());
    assert_eq!(
        check_failure(true, Some(&error), "Mic", true),
        Some(Failure::PermissionDenied)
    );
}

/// The real worker reports a missing device through its channel, with
/// callbacks flowing so that only the device list can fail the stream.
#[test]
fn the_worker_reports_a_missing_device() {
    use iced::futures::StreamExt;
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
    let mut watch = watch_of(&writer, &parts);
    watch.list = Some(|| inputs(&["Other"]));
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
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut failed = false;
    while !failed && Instant::now() < deadline {
        writer.callback_received();
        failed = receive.recv_timeout(Duration::from_millis(50)).is_ok();
    }
    assert!(failed, "the worker reported the loss");
    assert!(!writer.status().is_live());
}

/// The real worker checks the permission after the stream has started.
#[test]
fn the_worker_reports_a_late_permission_denial() {
    use iced::futures::StreamExt;
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
    let mut watch = watch_of(&writer, &parts);
    watch.permission_denied = Some(|| true);
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

/// The real worker fails a stream whose callbacks stop, while the device list
/// still shows the device, so only the stall can be the cause.
#[test]
fn the_worker_reports_a_stall() {
    use iced::futures::StreamExt;
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
    writer.callback_received();
    let mut watch = watch_of(&writer, &parts);
    watch.list = Some(|| inputs(&["Test input"]));
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
        .expect("the stall was reported");
    assert!(!writer.status().is_live());
}

/// The worker ends when its subscription ends, so no thread outlives it.
#[test]
fn the_worker_stops_with_its_subscription() {
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let (writer, _reader) = loom_core::audio_input::input_ring(32, 2);
    let watch = watch_of(&writer, &parts);
    let (tx, rx) = mpsc::channel(4);
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        run(&watch, tx);
        let _ = done.send(());
    });
    drop(rx);
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("the worker stopped");
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
        let mut watch = watch_of(&writer, &parts);
        watch.tracker = tracker;
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
