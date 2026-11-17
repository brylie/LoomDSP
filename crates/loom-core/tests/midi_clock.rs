//! MIDI transport and bounded clock egress on the engine timeline.

use loom_core::{ClockEvent, ClockMessage, EngineConfig, EngineParts, MidiEvent, MidiInputId, Registry, create_engine};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Creates an empty engine at 48 kHz.
fn engine() -> EngineParts {
    create_engine(Arc::new(Registry::default()), EngineConfig::with_sample_rate(48_000.0))
}

/// Sends a MIDI message at the given offset in the next buffer.
fn send(parts: &mut EngineParts, frame: u32, input: MidiInputId, bytes: &[u8], micros: Option<u64>) {
    let mut event = MidiEvent::from_bytes(frame, bytes).from_input(input);
    event.arrival_micros = micros;
    assert!(parts.midi.send(event));
}

/// Advances the engine without collecting samples.
fn step(parts: &mut EngineParts, frames: usize) {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    parts.engine.process(&mut left, &mut right);
}

/// Identical sample positions produce identical output frames at all block sizes.
#[test]
fn sends_clock_at_sample_frames() {
    let render = |block| {
        let mut parts = engine();
        let mut output = parts.engine.attach_clock_output();
        let mut events = Vec::new();
        let mut frame = 0;
        while frame < 48_000 {
            let n = usize::min(block, 48_000 - frame);
            step(&mut parts, n);
            events.extend(std::iter::from_fn(|| output.pop()));
            frame += n;
        }
        events
    };
    let events = render(1);
    assert_eq!(events, render(77));
    assert_eq!(events, render(512));
    assert_eq!(events.first().unwrap().message, ClockMessage::Start);
    let pulses: Vec<_> = events
        .iter()
        .filter(|event| event.message == ClockMessage::Pulse)
        .collect();
    assert_eq!(pulses.len(), 48);
    assert!(
        pulses
            .iter()
            .enumerate()
            .all(|(at, event)| event.frame.abs_diff(at as u64 * 1000) <= 1)
    );
}

/// Buffered pulses use their arrival timestamps, and only the selected input drives playback.
#[test]
fn follows_and_recovers_the_selected_input() {
    let mut parts = engine();
    let chosen = MidiInputId(1);
    let other = MidiInputId(2);
    parts.controller.open_midi_input(chosen).unwrap();
    parts.controller.open_midi_input(other).unwrap();
    parts.controller.set_tempo(90.0).unwrap();
    parts.controller.select_midi_clock(Some(chosen), false).unwrap();
    let mut output = parts.engine.attach_clock_output();
    send(&mut parts, 0, other, &[0xfa], None);
    send(&mut parts, 0, other, &[0xf8], None);
    step(&mut parts, 32);
    assert!(!output.state().1);
    send(&mut parts, 0, chosen, &[0xfa], None);
    send(&mut parts, 0, chosen, &[0xfc], None);
    send(&mut parts, 0, chosen, &[0xf8], Some(0));
    step(&mut parts, 32);
    assert!(!output.state().1, "Stop cancels a pending Start");
    send(&mut parts, 0, chosen, &[0xf2, 12, 0], None);
    send(&mut parts, 0, chosen, &[0xfb], None);
    for pulse in 1..=24 {
        send(&mut parts, 0, chosen, &[0xf8], Some(pulse * 500_000 / 24));
    }
    step(&mut parts, 32);
    assert!((parts.controller.rendered_tempo().unwrap() - 120.0).abs() < 0.1);
    let events: Vec<ClockEvent> = std::iter::from_fn(|| output.pop()).collect();
    assert_eq!(events[0].message, ClockMessage::Position(12));
    assert_eq!(events[1].message, ClockMessage::Position(12));
    assert_eq!(events[2].message, ClockMessage::Continue);
    assert!(events.iter().all(|event| event.frame == 64));
    let locked_at = output.state().0;
    assert!(output.state().1);
    step(&mut parts, 72_000);
    assert!(parts.controller.stats().clock_lost.load(Ordering::Relaxed));
    assert!(output.state().0 > locked_at + 0.5);
    send(&mut parts, 0, chosen, &[0xf8], Some(1_900_000));
    step(&mut parts, 32);
    assert!(!parts.controller.stats().clock_lost.load(Ordering::Relaxed));
    assert!((parts.controller.rendered_tempo().unwrap() - 120.0).abs() < 0.1);
    parts.controller.select_midi_clock(None, false).unwrap();
    step(&mut parts, 32);
    assert_eq!(parts.controller.rendered_tempo(), Some(90.0));
}

/// A running jump skips clock pulses and a stopped jump sends a position.
#[test]
fn output_orders_stop_locate_and_resume() {
    let mut parts = engine();
    let mut output = parts.engine.attach_clock_output();
    step(&mut parts, 500);
    assert_eq!(output.pop().unwrap().message, ClockMessage::Start);
    assert_eq!(output.pop().unwrap().message, ClockMessage::Pulse);
    parts.controller.set_running(false).unwrap();
    parts.controller.reset_transport().unwrap();
    step(&mut parts, 512);
    assert_eq!(output.pop().unwrap().message, ClockMessage::Stop);
    assert_eq!(output.pop().unwrap().message, ClockMessage::Position(0));
    assert!(output.pop().is_none());
    parts.controller.set_running(true).unwrap();
    step(&mut parts, 512);
    assert_eq!(output.pop().unwrap().message, ClockMessage::Start);
    assert_eq!(output.pop().unwrap().message, ClockMessage::Pulse);
    assert!(output.pop().is_none());
}
