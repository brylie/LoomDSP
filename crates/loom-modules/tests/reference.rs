//! Reference renders of every built-in DSP module (FR-SDK-02, NFR-OSS-05).
//!
//! Each test checks that block size does not change the output, then
//! compares with `tests/reference/<name>.wav`. After an intended change,
//! update the files with `LOOM_UPDATE_REFERENCES=1 cargo test -p loom-modules`,
//! listen to them, and commit them.

use loom_core::{MidiEvent, ModuleRole};
use loom_modules::{ALL, display, env, filter, level, logic, math, midi, osc, phase, time, ui};
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

/// Reference render for `loom.osc.pulse`.
#[test]
fn pulse() {
    Harness::new(&osc::PULSE)
        .seconds(0.05)
        .input("phase", Signal::phase(220.0))
        .param("width", 0.25)
        .check(reference!("pulse"));
}

/// Reference render for `loom.osc.triangle`.
#[test]
fn triangle() {
    Harness::new(&osc::TRIANGLE)
        .seconds(0.05)
        .input("phase", Signal::phase(220.0))
        .check(reference!("triangle"));
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

/// Reference render for `loom.level.amp`: exponential response, a falling level.
#[test]
fn amp() {
    Harness::new(&level::AMP)
        .seconds(0.02)
        .input("in", Signal::phase(100.0))
        .input("level", Signal::from_fn(|frame| 1.0 - frame as f32 / 960.0))
        .param("gain", 1.5)
        .param("response", 1.0)
        .check(reference!("amp"));
}

#[test]
fn constant() {
    Harness::new(&math::CONSTANT)
        .seconds(0.01)
        .param("value", 3.0)
        .check(reference!("constant"));
}

/// Reference render for `loom.math.convert`: pitch to Hz on a rising ramp.
#[test]
fn convert() {
    Harness::new(&math::CONVERT)
        .seconds(0.01)
        .input("in", Signal::from_fn(|frame| frame as f32 / 480.0 * 0.2 - 0.1))
        .param("mode", 2.0)
        .check(reference!("convert"));
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
fn meter() {
    // A pass-through: the reference is its input, a 220 Hz sine at half level.
    let render = Harness::new(&display::METER)
        .seconds(0.02)
        .input(
            "in",
            Signal::from_fn(|frame| 0.5 * (frame as f32 * 220.0 / 48_000.0 * std::f32::consts::TAU).sin()),
        )
        .check(reference!("meter"));
    let expected: Vec<f32> = (0..render.frames())
        .map(|frame| 0.5 * (frame as f32 * 220.0 / 48_000.0 * std::f32::consts::TAU).sin())
        .collect();
    assert_eq!(render.output("out"), expected.as_slice());
}

#[test]
fn scope() {
    // A pass-through, whatever its parameters: they only tell the app how to
    // draw. The reference is its input, a 220 Hz sine at half level.
    let sine = |frame: usize| 0.5 * (frame as f32 * 220.0 / 48_000.0 * std::f32::consts::TAU).sin();
    let render = Harness::new(&display::SCOPE)
        .seconds(0.02)
        .param("window", 0.5)
        .param("auto", 0.0)
        .param("level", 0.3)
        .input("in", Signal::from_fn(sine))
        .check(reference!("scope"));
    let expected: Vec<f32> = (0..render.frames()).map(sine).collect();
    assert_eq!(render.output("out"), expected.as_slice());
}

#[test]
fn divide() {
    // A trigger every 10 ms, divided by 3 with offset 1, reset at 55 ms.
    let render = Harness::new(&logic::DIVIDE)
        .seconds(0.1)
        .input(
            "trigger",
            Signal::from_fn(|frame| if frame % 480 == 0 { 1.0 } else { 0.0 }),
        )
        .input("reset", Signal::gate(0.055, 0.056))
        .param("n", 3.0)
        .param("offset", 1.0)
        .check(reference!("divide"));
    let fired: Vec<usize> = render
        .output("trigger")
        .iter()
        .enumerate()
        .filter(|&(_, &value)| value == 1.0)
        .map(|(frame, _)| frame / 480 + 1)
        .collect();
    // Triggers 2 and 5 fire. The reset falls before trigger 7, which then
    // counts as the first again, so 8 fires.
    assert_eq!(fired, [2, 5, 8]);
}

#[test]
fn phase_trigger() {
    // A 40 Hz phase from a bar line, so it fires on frame 0 and then every
    // 1200 frames.
    let render = Harness::new(&phase::PHASE_TRIGGER)
        .seconds(0.1)
        .input("phase", Signal::phase(40.0))
        .check(reference!("phase_trigger"));
    let fired: Vec<usize> = render
        .output("trigger")
        .iter()
        .enumerate()
        .filter(|&(_, &value)| value == 1.0)
        .map(|(frame, _)| frame)
        .collect();
    assert_eq!(fired, [0, 1_200, 2_400, 3_600]);
}

#[test]
fn button() {
    // The gate is the `on` parameter, held for the whole render.
    let render = Harness::new(&ui::BUTTON)
        .seconds(0.02)
        .param("on", 1.0)
        .check(reference!("button"));
    assert!(render.output("gate").iter().all(|&sample| sample == 1.0));
}

/// Reference render for `loom.time.delay`.
#[test]
fn delay() {
    // A 220 Hz sine with a slow ±2 ms modulation around a 15 ms delay.
    Harness::new(&time::DELAY)
        .seconds(0.1)
        .input(
            "in",
            Signal::from_fn(|frame| 0.5 * (frame as f32 * 220.0 / 48_000.0 * std::f32::consts::TAU).sin()),
        )
        .input(
            "time",
            Signal::from_fn(|frame| 0.002 * (frame as f32 * 5.0 / 48_000.0 * std::f32::consts::TAU).sin()),
        )
        .param("time", 0.015)
        .check(reference!("delay"));
}

/// Reference render for `loom.time.allpass`.
#[test]
fn allpass() {
    // A short 330 Hz tone burst diffused through a 7 ms Schroeder all-pass
    // with ±1 ms modulation.
    Harness::new(&time::ALLPASS)
        .seconds(0.1)
        .input(
            "in",
            Signal::from_fn(|frame| {
                if frame < 480 {
                    0.5 * (frame as f32 * 330.0 / 48_000.0 * std::f32::consts::TAU).sin()
                } else {
                    0.0
                }
            }),
        )
        .input(
            "time",
            Signal::from_fn(|frame| 0.001 * (frame as f32 * 7.0 / 48_000.0 * std::f32::consts::TAU).sin()),
        )
        .param("time", 0.007)
        .param("gain", 0.6)
        .check(reference!("allpass"));
}

#[test]
fn every_dsp_module_has_a_reference_test() {
    // Add a test above when you add a module to `ALL`.
    let tested = [
        &phase::PHASE,
        &phase::PHASE_TRIGGER,
        &osc::SINE,
        &osc::SAW,
        &osc::PULSE,
        &osc::TRIANGLE,
        &osc::NOISE,
        &filter::SVF,
        &env::ADSR,
        &math::ADD,
        &math::MUL,
        &math::CONSTANT,
        &math::CONVERT,
        &level::AMP,
        &midi::NOTE_IN,
        &display::METER,
        &display::SCOPE,
        &logic::DIVIDE,
        &ui::BUTTON,
        &time::DELAY,
        &time::ALLPASS,
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
