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

/// Built-in tiles use knobs for single continuous values. A read-only shape
/// or a named choice does not change that input grammar.
#[test]
fn continuous_panel_controls_are_knobs() {
    for descriptor in loom_modules::ALL {
        for widget in descriptor.panel.widgets.iter().filter(|widget| widget.is_control()) {
            let param = &descriptor.params[descriptor.param_index(widget.bind).unwrap()];
            if !param.options.integer {
                assert_eq!(
                    widget.kind,
                    loom_core::WidgetKind::Knob,
                    "{}.{}",
                    descriptor.id,
                    widget.bind
                );
            }
        }
    }
}

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
fn envelope_lights_follow_their_levels() {
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

/// A Clock Divide and a Phase Trigger fed by a MIDI Note gate, with the
/// divide firing on every trigger. The gate's rise is a one-sample trigger
/// out of both, and its fall is one more out of the Phase Trigger.
fn trigger_lights() -> (EngineParts, ModuleId, ModuleId) {
    let mut parts = loom_core::create_engine(
        loom_modules::registry(),
        loom_core::EngineConfig::with_sample_rate(SAMPLE_RATE),
    );
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let divide = c.add_module("loom.logic.divide").unwrap();
    let phase = c.add_module("loom.phase.trigger").unwrap();
    c.set_param_named(divide, "n", 1.0).unwrap();
    c.connect_named(note, "gate", divide, "trigger").unwrap();
    c.connect_named(note, "gate", phase, "phase").unwrap();
    c.commit().unwrap();
    (parts, divide, phase)
}

/// The reads of a trace that saw the light lit, as seconds.
fn lit(trace: &Trace) -> Vec<f32> {
    trace
        .iter()
        .filter(|&&(_, value)| value == 1.0)
        .map(|&(frame, _)| frame as f32 / SAMPLE_RATE)
        .collect()
}

/// A one-sample trigger lights a trigger light at the next read, at every
/// block size, though the latest value is 0 by then (panels.md, Lights for
/// one-sample triggers). Each trigger is seen by exactly one read.
#[test]
fn one_sample_triggers_light_trigger_lights() {
    for block in BLOCKS {
        let (mut parts, divide, _) = trigger_lights();
        let fired = watch(&mut parts, FRAMES, block, &note(), divide, "fired");
        let seen = lit(&fired);
        assert_eq!(seen.len(), 1, "block {block}: {seen:?}");
        assert!(
            seen[0] > NOTE_ON && seen[0] <= NOTE_ON + 512.0 / SAMPLE_RATE + 1e-6,
            "block {block}: {seen:?}"
        );

        let (mut parts, _, phase) = trigger_lights();
        let fired = watch(&mut parts, FRAMES, block, &note(), phase, "fired");
        // The first frame reads phase 0, which counts as a wrap. Then the
        // gate's rise and fall.
        let seen = lit(&fired);
        assert_eq!(seen.len(), 3, "block {block}: {seen:?}");
        assert!(seen[1] > NOTE_ON && seen[2] > NOTE_OFF, "block {block}: {seen:?}");
    }
}
