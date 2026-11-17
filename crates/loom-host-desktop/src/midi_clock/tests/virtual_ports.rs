use super::*;
use crate::{AudioOutput, VirtualController, connect_midi_input, share_midi};
use loom_core::MidiInputId;
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
