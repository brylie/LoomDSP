//! What the panels of the subtractive example show while a note plays
//! (`docs/design/panels.md`, acceptance criteria; issue #92).
//!
//! These checks were manual: run the app, hold a note, and watch the lights
//! and meters. Here the same patch plays the same note headlessly, and the
//! tests read what the app would draw from: display values for lights, and
//! meter readings for meters. Each runs at host buffer sizes 1, 77, and 512,
//! as the module harness does, because display values and meter readings
//! are published once per block.

mod common;

use common::*;
use loom_core::{EngineParts, ModuleId, ProbeId, ProbeKind};

/// Module IDs in `examples/patches/subtractive.ron`.
const MIDI_NOTE: ModuleId = ModuleId(1);
const FILTER: ModuleId = ModuleId(4);
const FILTER_ENVELOPE: ModuleId = ModuleId(5);
const AMP_ENVELOPE: ModuleId = ModuleId(7);
const VCA: ModuleId = ModuleId(8);

const BLOCKS: [usize; 3] = [1, 77, 512];

/// Two and a half seconds, with a note held from 0.1 s to 1.0 s.
const FRAMES: usize = 120_000;
const NOTE_ON: f32 = 0.1;
const NOTE_OFF: f32 = 1.0;

fn note() -> [Note; 1] {
    [Note {
        note: 57,
        on: (NOTE_ON * SAMPLE_RATE) as usize,
        off: (NOTE_OFF * SAMPLE_RATE) as usize,
    }]
}

#[test]
fn midi_note_gate_light_follows_the_held_note() {
    for block in BLOCKS {
        let mut parts = load("subtractive.ron");
        let gate = watch(&mut parts, FRAMES, block, &note(), MIDI_NOTE, "gate");
        assert_eq!(at(&gate, 0.05), 0.0, "block {block}: dark before the note");
        assert_eq!(at(&gate, 0.5), 1.0, "block {block}: lit while the note is held");
        assert_eq!(at(&gate, 1.5), 0.0, "block {block}: dark after the note");
    }
}

#[test]
fn adsr_lights_follow_their_envelopes() {
    // (module, sustain level in the patch)
    for (envelope, sustain) in [(AMP_ENVELOPE, 0.7), (FILTER_ENVELOPE, 0.2)] {
        for block in BLOCKS {
            let mut parts = load("subtractive.ron");
            let level = watch(&mut parts, FRAMES, block, &note(), envelope, "level");
            let at = |seconds| at(&level, seconds);
            let case = format!("{envelope:?}, block {block}");
            assert_eq!(at(0.05), 0.0, "{case}: dark before the note");
            assert!(
                max_between(&level, NOTE_ON, NOTE_ON + 0.1) > 0.9,
                "{case}: the attack lights it fully"
            );
            assert!(
                (at(0.9) - sustain).abs() < 0.01,
                "{case}: holds at sustain {sustain}, got {}",
                at(0.9)
            );
            // Release: it fades, never jumps up, and goes dark.
            let fading = at(NOTE_OFF + 0.05);
            assert!(0.0 < fading && fading < sustain, "{case}: fading, got {fading}");
            let after: Vec<f32> = level
                .iter()
                .filter(|&&(f, _)| f as f32 >= NOTE_OFF * SAMPLE_RATE)
                .map(|&(_, v)| v)
                .collect();
            assert!(after.windows(2).all(|w| w[1] <= w[0]), "{case}: the release only falls");
            assert_eq!(at(2.4), 0.0, "{case}: dark after the release");
        }
    }
}

/// Adds a Level Meter fed from `output` of `module`, and a meter probe on
/// its output, as the app does for a meter on screen.
fn add_level_meter(parts: &mut EngineParts, module: ModuleId, output: &str) -> ProbeId {
    let meter = parts.controller.add_module("loom.display.meter").unwrap();
    parts.controller.connect_named(module, output, meter, "in").unwrap();
    let out = parts.controller.output_ref(meter, "out").unwrap();
    let probe = parts.controller.add_probe(out, ProbeKind::Meter).unwrap();
    parts.controller.commit().unwrap();
    probe
}

#[test]
fn a_level_meter_after_the_vca_rises_with_a_note_and_falls_after_it() {
    for block in BLOCKS {
        let mut parts = load("subtractive.ron");
        let probe = add_level_meter(&mut parts, VCA, "out");
        let readings = meter_readings(&mut parts, FRAMES, block, &note());
        let level = |from, to| level_between(&readings, probe, from, to);

        assert_eq!(level(0.0, NOTE_ON), (0.0, 0.0), "block {block}: silent before the note");
        // About -18 dB RMS while held: well clear of the -60 dB floor.
        let (peak, rms) = level(0.3, NOTE_OFF);
        assert!(
            rms > 0.05 && peak > 0.1,
            "block {block}: up while held, got {peak} {rms}"
        );
        // The envelope ends at exactly 0, so the VCA's output does too.
        assert_eq!(level(2.0, 2.5), (0.0, 0.0), "block {block}: silent after the release");
    }
}

#[test]
fn a_level_meter_on_the_filter_output_shows_the_drone() {
    // The saw runs all the time and only the VCA is gated, so the filter's
    // output is never silent. A meter there stays up with no note held.
    // This is correct, and it is why issue #92's check moved after the VCA.
    for block in BLOCKS {
        let mut parts = load("subtractive.ron");
        let probe = add_level_meter(&mut parts, FILTER, "lowpass");
        let readings = meter_readings(&mut parts, FRAMES, block, &note());
        let level = |from, to| level_between(&readings, probe, from, to);
        // About -5 dB RMS before, during, and after the note.
        for (from, to) in [(0.0, NOTE_ON), (0.3, NOTE_OFF), (2.0, 2.5)] {
            let (_, rms) = level(from, to);
            assert!(rms > 0.3, "block {block}: up from {from} s to {to} s, got {rms}");
        }
    }
}
