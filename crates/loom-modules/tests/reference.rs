//! Reference renders of every built-in DSP module (FR-SDK-02, NFR-OSS-05).
//!
//! Each test checks that block size does not change the output, then
//! compares with `tests/reference/<name>.wav`. After an intended change,
//! update the files with `LOOM_UPDATE_REFERENCES=1 cargo test -p loom-modules`,
//! listen to them, and commit them.

use loom_core::{MidiEvent, ModuleRole};
use loom_modules::{ALL, env, filter, math, midi, osc, phase};
use loom_test::{Harness, Signal, reference};

#[test]
fn phase() {
    Harness::new(&phase::PHASE)
        .seconds(0.1)
        // Middle C, a fifth up, gliding up an octave.
        .input("pitch", Signal::from_fn(|frame| frame as f32 / 4_800.0 * 0.1))
        .input("reset", Signal::gate(0.05, 0.06))
        .param("tune", 7.0)
        .check(reference!("phase"));
}

#[test]
fn sine() {
    Harness::new(&osc::SINE)
        .seconds(0.05)
        .input("phase", Signal::phase(440.0))
        .check(reference!("sine"));
}

#[test]
fn saw() {
    Harness::new(&osc::SAW)
        .seconds(0.05)
        .input("phase", Signal::phase(220.0))
        .check(reference!("saw"));
}

#[test]
fn noise() {
    Harness::new(&osc::NOISE)
        .seconds(0.05)
        .param("seed", 7.0)
        .check(reference!("noise"));
}

#[test]
fn svf() {
    Harness::new(&filter::SVF)
        .seconds(0.1)
        // A 120 Hz square wave.
        .input(
            "in",
            Signal::from_fn(|frame| if (frame / 200) % 2 == 0 { 0.5 } else { -0.5 }),
        )
        // Sweeps the cutoff up two octaves.
        .input("cutoff", Signal::from_fn(|frame| frame as f32 / 4_800.0 * 0.2))
        .param("cutoff", 400.0)
        .param("resonance", 0.6)
        .check(reference!("svf"));
}

#[test]
fn adsr() {
    Harness::new(&env::ADSR)
        .seconds(0.2)
        .input("gate", Signal::gate(0.01, 0.12))
        .param("attack", 0.02)
        .param("decay", 0.03)
        .param("sustain", 0.5)
        .param("release", 0.05)
        .check(reference!("adsr"));
}

#[test]
fn add() {
    Harness::new(&math::ADD)
        .seconds(0.02)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::constant(0.25))
        .param("offset", 0.5)
        .check(reference!("add"));
}

#[test]
fn mul() {
    Harness::new(&math::MUL)
        .seconds(0.02)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::constant(0.5))
        .param("gain", 2.0)
        .check(reference!("mul"));
}

#[test]
fn constant() {
    Harness::new(&math::CONSTANT)
        .seconds(0.01)
        .param("value", 3.0)
        .check(reference!("constant"));
}

#[test]
fn note_in() {
    Harness::new(&midi::NOTE_IN)
        .seconds(0.1)
        .midi([
            MidiEvent::note_on(100, 0, 60, 100),
            MidiEvent::note_on(1_000, 0, 67, 64),
            // Pitch bend halfway up.
            MidiEvent::from_bytes(2_000, &[0xE0, 0x00, 0x60]),
            MidiEvent::note_off(3_000, 0, 67),
            MidiEvent::note_off(4_000, 0, 60),
        ])
        .check(reference!("note_in"));
}

#[test]
fn every_dsp_module_has_a_reference_test() {
    // Add a test above when you add a module to `ALL`.
    let tested = [
        &phase::PHASE,
        &osc::SINE,
        &osc::SAW,
        &osc::NOISE,
        &filter::SVF,
        &env::ADSR,
        &math::ADD,
        &math::MUL,
        &math::CONSTANT,
        &midi::NOTE_IN,
    ]
    .map(|descriptor| descriptor.id);
    for descriptor in ALL {
        if matches!(descriptor.role, ModuleRole::Dsp(_)) {
            assert!(
                tested.contains(&descriptor.id),
                "{} has no reference test",
                descriptor.id
            );
        }
    }
}
