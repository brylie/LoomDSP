//! Held notes belong to a MIDI input (`docs/design/device-changes.md`,
//! MIDI inputs): the engine releases what an input held when it goes away.

mod common;

use common::*;
use loom_core::{EngineConfig, EngineParts, MidiEvent, MidiInputId, OutputSafety, create_engine};

const KEYS: MidiInputId = MidiInputId(1);
const PAD: MidiInputId = MidiInputId(2);

/// MIDI Note's gate into Audio Out, with no safety stage, so the output is
/// the gate.
fn gate() -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(note, "gate", out, "in").unwrap();
    c.commit().unwrap();
    // Past the fade-in.
    run(&mut parts);
    parts
}

/// Plays a buffer and returns its last sample: the gate.
fn run(parts: &mut EngineParts) -> f32 {
    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];
    parts.engine.process(&mut left, &mut right);
    left[1023]
}

/// Sends a note on or off from `input`.
fn send(parts: &mut EngineParts, input: MidiInputId, on: bool) {
    let event = if on {
        MidiEvent::note_on(0, 0, 60, 100)
    } else {
        MidiEvent::note_off(0, 0, 60)
    };
    assert!(parts.midi.send(event.from_input(input)));
}

/// A note held on an input that goes away is released, and the engine
/// holds nothing for it.
#[test]
fn removing_an_input_releases_its_notes() {
    let mut parts = gate();
    parts.controller.open_midi_input(KEYS).unwrap();
    send(&mut parts, KEYS, true);
    assert_eq!(run(&mut parts), 1.0, "the note plays");
    assert_eq!(parts.engine.held_notes(KEYS), Some(1));
    parts.controller.release_midi_input(KEYS).unwrap();
    assert_eq!(run(&mut parts), 0.0, "a note off reached the modules");
    assert_eq!(parts.engine.held_notes(KEYS), None, "nothing is held for it");
    // A late event from the closed input is dropped.
    send(&mut parts, KEYS, true);
    assert_eq!(run(&mut parts), 0.0);
}

/// Two inputs holding the same note: removing one keeps the note on until
/// the other releases it.
#[test]
fn a_note_ends_with_its_last_holder() {
    let mut parts = gate();
    parts.controller.open_midi_input(KEYS).unwrap();
    parts.controller.open_midi_input(PAD).unwrap();
    send(&mut parts, KEYS, true);
    send(&mut parts, PAD, true);
    assert_eq!(run(&mut parts), 1.0);
    parts.controller.release_midi_input(KEYS).unwrap();
    assert_eq!(run(&mut parts), 1.0, "the pad still holds it");
    send(&mut parts, PAD, false);
    assert_eq!(run(&mut parts), 0.0, "the last holder let go");
}

/// An input opened again, many times quickly, never leaves a note stuck
/// and never doubles one.
#[test]
fn quick_unplug_and_replug_leaves_no_stuck_note() {
    let mut parts = gate();
    for _ in 0..10 {
        parts.controller.open_midi_input(KEYS).unwrap();
        send(&mut parts, KEYS, true);
        assert_eq!(run(&mut parts), 1.0);
        parts.controller.release_midi_input(KEYS).unwrap();
        assert_eq!(run(&mut parts), 0.0);
    }
    parts.controller.open_midi_input(KEYS).unwrap();
    send(&mut parts, KEYS, true);
    assert_eq!(run(&mut parts), 1.0);
    // Opening it again while a note is held starts clean: the note ends.
    parts.controller.open_midi_input(KEYS).unwrap();
    assert_eq!(run(&mut parts), 0.0);
    assert_eq!(parts.engine.held_notes(KEYS), Some(0));
}

/// An id out of range is refused.
#[test]
fn input_ids_are_checked() {
    let mut parts = gate();
    assert!(parts.controller.open_midi_input(MidiInputId(0)).is_err());
    let max = MidiInputId(loom_core::MAX_MIDI_INPUTS);
    assert!(parts.controller.release_midi_input(max).is_err());
}
