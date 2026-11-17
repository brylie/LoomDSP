use super::*;
use loom_core::{EngineConfig, create_engine};

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod virtual_ports;

/// A fake receiver locates before Continue and skips an overdue pulse backlog.
#[test]
fn recovering_output_sends_state_before_pulses() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let first = Instant::now();
    let (_, anchors) = RingBuffer::new(1);
    let reader = parts.engine.attach_clock_output();
    let mut worker = Worker::new(reader, anchors, 48_000.0, ClockPorts::default());
    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = messages.clone();
    worker.connections.push(Connection {
        endpoint: Endpoint {
            name: "Test receiver".into(),
            id: "test".into(),
        },
        _lifetime: Arc::new(()),
        needs_state: true,
        send_message: Box::new(move |message| {
            received.lock().unwrap().push(message);
            true
        }),
    });
    worker.anchors.push_back(Anchor {
        frame: 0,
        playback: first + Duration::from_millis(8),
    });
    parts.engine.process(&mut [0.0; 2048], &mut [0.0; 2048]);
    worker.dispatch(|| first);
    assert!(messages.lock().unwrap().is_empty(), "silence until playback time");
    worker.dispatch(|| first + Duration::from_millis(8));
    assert_eq!(&*messages.lock().unwrap(), &[ClockMessage::Start, ClockMessage::Pulse]);
    worker.dispatch(|| first + Duration::from_millis(100));
    assert_eq!(
        &*messages.lock().unwrap(),
        &[ClockMessage::Start, ClockMessage::Pulse, ClockMessage::Stop]
    );
    assert_eq!(worker.ports.failures(), 1);
    let resumed = first + Duration::from_millis(101);
    worker.anchors.push_back(Anchor {
        frame: 2048,
        playback: resumed,
    });
    parts.engine.process(&mut [0.0; 1024], &mut [0.0; 1024]);
    worker.dispatch(|| resumed + Duration::from_millis(20));
    assert_eq!(
        &messages.lock().unwrap()[3..],
        &[ClockMessage::Position(0), ClockMessage::Continue, ClockMessage::Pulse]
    );
}

/// A worker without any routed outputs still drains callback anchors and shuts down cleanly.
#[test]
fn unrouted_worker_drains_playback_anchors() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let ports = ClockPorts::default();
    let (worker, mut anchors) = ClockWorker::spawn(&mut parts.engine, ports.clone()).unwrap();
    let capacity = anchors.slots();
    anchors
        .push(Anchor {
            frame: 0,
            playback: Instant::now(),
        })
        .unwrap_or_else(|_| panic!("new ring has room"));
    let until = Instant::now() + Duration::from_secs(2);
    while anchors.slots() != capacity && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(anchors.slots(), capacity, "the worker drained the callback anchor");
    drop(worker);
    assert_eq!(ports.failures(), 0);
}

/// Turning off the last output stops its receiver and cancels queued clock messages.
#[test]
fn disabling_the_last_output_stops_the_receiver() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (_, anchors) = RingBuffer::new(1);
    let mut worker = Worker::new(
        parts.engine.attach_clock_output(),
        anchors,
        48_000.0,
        ClockPorts::default(),
    );
    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = messages.clone();
    worker.connections.push(Connection {
        endpoint: Endpoint {
            name: "Synth".into(),
            id: "test".into(),
        },
        _lifetime: Arc::new(()),
        needs_state: false,
        send_message: Box::new(move |message| {
            received.lock().unwrap().push(message);
            true
        }),
    });

    parts.engine.process(&mut [0.0; 128], &mut [0.0; 128]);
    worker.dispatch(Instant::now);
    assert!(worker.pending.is_some(), "the first message has no playback anchor yet");
    worker.apply_scan(prepare_scan(Vec::new(), &mut Vec::new()));
    assert_eq!(&*messages.lock().unwrap(), &[ClockMessage::Stop]);
    assert!(worker.connections.is_empty());
    assert!(worker.pending.is_none());
    parts.engine.process(&mut [0.0; 2048], &mut [0.0; 2048]);
    assert!(worker.reader.pop().is_none(), "the engine stopped producing events");
}

/// A live receiver that loses its numbered name stops. Unchanged outputs keep playing.
#[test]
fn reconciliation_stops_renamed_receiver_only() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (_, anchors) = RingBuffer::new(1);
    let mut worker = Worker::new(
        parts.engine.attach_clock_output(),
        anchors,
        48_000.0,
        ClockPorts::default(),
    );
    let (sent, received) = mpsc::channel();
    let renamed = Endpoint {
        name: "Synth (2)".into(),
        id: "second".into(),
    };
    let unchanged = Endpoint {
        name: "Drums".into(),
        id: "drums".into(),
    };
    for endpoint in [renamed.clone(), unchanged.clone()] {
        let sent = sent.clone();
        let id = endpoint.id.clone();
        worker.connections.push(Connection {
            endpoint,
            _lifetime: Arc::new(()),
            needs_state: false,
            send_message: Box::new(move |message| sent.send((id.clone(), message)).is_ok()),
        });
    }

    // The first Synth disappeared. Its surviving peer now has the bare name.
    worker.apply_scan(Scan {
        wanted: vec![renamed.name, unchanged.name.clone()],
        available: Some(vec![
            Endpoint {
                name: "Synth".into(),
                id: renamed.id,
            },
            unchanged.clone(),
        ]),
        ..Scan::default()
    });
    assert_eq!(
        received.try_iter().collect::<Vec<_>>(),
        [("second".into(), ClockMessage::Stop)]
    );
    assert_eq!(worker.connections.len(), 1);
    assert!(worker.connections[0].endpoint == unchanged);
    assert!(!worker.connections[0].needs_state);
}
