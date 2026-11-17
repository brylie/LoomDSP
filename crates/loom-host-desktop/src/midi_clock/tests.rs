use super::*;
use crate::{AudioOutput, VirtualController, connect_midi_input, share_midi};
use loom_core::{EngineConfig, MidiInputId, create_engine};
use midir::{MidiInput, os::unix::VirtualInput};
use std::sync::mpsc;

/// A second MIDI client starts, clocks, and stops LoomDSP and records its output.
#[test]
#[ignore = "needs the system MIDI service with virtual ports"]
fn follows_and_sends_via_virtual_ports() {
    let name = format!("Loom sink {}", std::process::id());
    let (send, receive) = mpsc::channel();
    let _sink = MidiInput::new("Loom clock check")
        .unwrap()
        .create_virtual(
            &name,
            |_, bytes, send| {
                let _ = send.send(bytes.to_vec());
            },
            send,
        )
        .unwrap();
    let output_name = (0..100)
        .find_map(|_| {
            thread::sleep(Duration::from_millis(10));
            midi_clock_outputs()
                .unwrap()
                .into_iter()
                .find(|port| port.contains(&name))
        })
        .expect("the sink is available as a MIDI output");
    let mut source = VirtualController::create(&format!("Loom source {}", std::process::id())).unwrap();
    let input_name = source.input_name().unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let input = MidiInputId(1);
    parts.controller.open_midi_input(input).unwrap();
    parts.controller.select_midi_clock(Some(input), false).unwrap();
    let _source_input = connect_midi_input(&input_name, share_midi(parts.midi), input).unwrap();
    let enabled = ClockPorts::default();
    enabled.set_enabled(&[output_name]);
    let mut output = AudioOutput::clock(48_000, 256);
    output.set_midi_clock(enabled);
    let _audio = output.start(parts.engine).map_err(|error| error.error).unwrap();
    assert_eq!(receive.recv_timeout(Duration::from_secs(2)).unwrap(), [0xfc]);

    source.send(&[0xfa]).unwrap();
    let start = Instant::now();
    for pulse in 0..30 {
        let due = start + Duration::from_micros(pulse * 500_000 / 24);
        thread::sleep(due.saturating_duration_since(Instant::now()));
        source.send(&[0xf8]).unwrap();
    }
    source.send(&[0xfc]).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    let mut messages = Vec::new();
    while let Ok(bytes) = receive.recv_timeout(until.saturating_duration_since(Instant::now())) {
        let stop = bytes == [0xfc];
        messages.push(bytes);
        if stop {
            break;
        }
    }
    assert!(messages.contains(&vec![0xfa]), "{messages:?}");
    assert!(
        messages.iter().filter(|bytes| bytes.as_slice() == [0xf8]).count() >= 24,
        "{messages:?}"
    );
    assert_eq!(messages.last().unwrap(), &[0xfc]);
    assert!((parts.controller.rendered_tempo().unwrap() - 120.0).abs() < 2.0);
}

/// A fake receiver locates before Continue and skips an overdue pulse backlog.
#[test]
fn recovering_output_sends_state_before_pulses() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let first = Instant::now();
    let (_, anchors) = RingBuffer::new(1);
    let reader = parts.engine.attach_clock_output();
    let mut worker = Worker::new(reader, anchors, 48_000.0, ClockPorts::default());
    let messages = Rc::new(RefCell::new(Vec::new()));
    let received = messages.clone();
    worker.connections.push(Connection {
        name: "Test receiver".into(),
        id: "test".into(),
        needs_state: true,
        send_message: Box::new(move |message| {
            received.borrow_mut().push(message);
            true
        }),
    });
    worker.anchors.push_back(Anchor {
        frame: 0,
        playback: first + Duration::from_millis(8),
    });
    parts.engine.process(&mut [0.0; 2048], &mut [0.0; 2048]);
    worker.dispatch(|| first);
    assert!(messages.borrow().is_empty(), "silence until playback time");
    worker.dispatch(|| first + Duration::from_millis(8));
    assert_eq!(&*messages.borrow(), &[ClockMessage::Start, ClockMessage::Pulse]);
    worker.dispatch(|| first + Duration::from_millis(100));
    assert_eq!(
        &*messages.borrow(),
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
        &messages.borrow()[3..],
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
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (_, anchors) = RingBuffer::new(1);
    let mut worker = Worker::new(
        parts.engine.attach_clock_output(),
        anchors,
        48_000.0,
        ClockPorts::default(),
    );
    let messages = Rc::new(RefCell::new(Vec::new()));
    let received = messages.clone();
    worker.connections.push(Connection {
        name: "Synth".into(),
        id: "test".into(),
        needs_state: false,
        send_message: Box::new(move |message| {
            received.borrow_mut().push(message);
            true
        }),
    });

    parts.engine.process(&mut [0.0; 128], &mut [0.0; 128]);
    worker.dispatch(Instant::now);
    assert!(worker.pending.is_some(), "the first message has no playback anchor yet");
    worker.scan();
    assert_eq!(&*messages.borrow(), &[ClockMessage::Stop]);
    assert!(worker.connections.is_empty());
    assert!(worker.pending.is_none());
    parts.engine.process(&mut [0.0; 2048], &mut [0.0; 2048]);
    assert!(worker.reader.pop().is_none(), "the engine stopped producing events");
}
