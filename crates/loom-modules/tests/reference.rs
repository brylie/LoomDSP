//! Reference renders of every built-in DSP module (FR-SDK-02, NFR-OSS-05).
//!
//! Each test checks that block size does not change the output, then
//! compares with `tests/reference/<name>.wav`. After an intended change,
//! update the files with `LOOM_UPDATE_REFERENCES=1 cargo test -p loom-modules`,
//! listen to them, and commit them.

use loom_core::{MidiEvent, ModuleRole, TransportSettings};
use loom_modules::{
    ALL, clock, display, env, filter, level, lfo, logic, math, midi, mix, osc, phase, rand, time, transport, ui,
};
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

/// Reference render for `loom.filter.onepole`: a square wave through a
/// cutoff that sweeps up an octave.
#[test]
fn onepole() {
    Harness::new(&filter::ONEPOLE)
        .seconds(0.1)
        .input(
            "in",
            Signal::from_fn(|frame| if (frame / 200) % 2 == 0 { 0.5 } else { -0.5 }),
        )
        .input("cutoff", Signal::from_fn(|frame| frame as f32 / 4_800.0 * 0.1))
        .param("cutoff", 300.0)
        .check(reference!("onepole"));
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

/// Reference render for `loom.math.sub`: a phase ramp minus a constant.
#[test]
fn sub() {
    Harness::new(&math::SUB)
        .seconds(0.02)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::constant(0.25))
        .check(reference!("sub"));
}

/// Reference render for `loom.math.div`: B is 0 for the first 100 frames
/// (output 0), then 2 (halves A).
#[test]
fn div() {
    Harness::new(&math::DIV)
        .seconds(0.02)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::from_fn(|frame| if frame < 100 { 0.0 } else { 2.0 }))
        .check(reference!("div"));
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
        // Hz values reach hundreds, so one float step differs between maths libraries.
        .tolerance(1e-3)
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
        &filter::ONEPOLE,
        &phase::WRAP,
        &math::PITCH_CONSTANT,
        &level::CONSTANT,
        &level::CLAMP,
        &level::SCALE,
        &mix::MIXER,
        &mix::CROSSFADE,
        &logic::COMPARE,
        &logic::LOGIC,
        &logic::MEMORY,
        &rand::SAMPLE_HOLD,
        &level::SLEW,
        &phase::SCALE,
        &lfo::LFO,
        &env::ADSR,
        &math::ADD,
        &math::MUL,
        &math::SUB,
        &math::DIV,
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
        &clock::CLOCK,
        &transport::TRANSPORT,
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

/// Reference render for `loom.phase.wrap`: a ramp from -1 to 3 wrapped.
#[test]
fn phase_wrap() {
    Harness::new(&phase::WRAP)
        .seconds(0.02)
        .input("in", Signal::from_fn(|frame| frame as f32 / 240.0 - 1.0))
        .check(reference!("phase_wrap"));
}

/// Reference render for `loom.phase.wrap` in fold mode.
#[test]
fn phase_fold() {
    Harness::new(&phase::WRAP)
        .seconds(0.02)
        .input("in", Signal::from_fn(|frame| frame as f32 / 240.0 - 1.0))
        .param("mode", 1.0)
        .check(reference!("phase_fold"));
}

/// Reference render for `loom.pitch.constant`.
#[test]
fn pitch_constant() {
    Harness::new(&math::PITCH_CONSTANT)
        .seconds(0.01)
        .param("semitones", 7.0)
        .check(reference!("pitch_constant"));
}

/// Reference render for `loom.level.clamp`: a sine limited to +-0.5.
#[test]
fn clamp() {
    Harness::new(&level::CLAMP)
        .seconds(0.02)
        .input(
            "in",
            Signal::from_fn(|frame| (frame as f32 / 480.0 * std::f32::consts::TAU).sin()),
        )
        .param("low", -0.5)
        .param("high", 0.5)
        .check(reference!("clamp"));
}

/// Reference render for `loom.level.scale`: bipolar to unipolar.
#[test]
fn scale() {
    Harness::new(&level::SCALE)
        .seconds(0.02)
        .input(
            "in",
            Signal::from_fn(|frame| (frame as f32 / 480.0 * std::f32::consts::TAU).sin()),
        )
        .param("factor", 0.5)
        .param("offset", 0.5)
        .check(reference!("scale"));
}

/// Reference render for `loom.mix.mixer`: three inputs, one muted.
#[test]
fn mixer() {
    Harness::new(&mix::MIXER)
        .seconds(0.02)
        .input("in1", Signal::phase(100.0))
        .input("in2", Signal::constant(0.25))
        .input("in3", Signal::constant(0.5))
        .param("level1", 0.5)
        .param("level2", 2.0)
        .param("mute3", 1.0)
        .check(reference!("mixer"));
}

/// Reference render for `loom.mix.crossfade`: equal power at mix 0.25.
#[test]
fn crossfade() {
    Harness::new(&mix::CROSSFADE)
        .seconds(0.02)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::constant(0.5))
        .param("mix", 0.25)
        .check(reference!("crossfade"));
}

/// Reference render for `loom.level.constant`: -6 dB.
#[test]
fn level_constant() {
    Harness::new(&level::CONSTANT)
        .seconds(0.01)
        .param("level", -6.0)
        .check(reference!("level_constant"));
}

/// A 100 Hz triangle-ish ramp for the gate modules, 0 to 1.
fn ramp() -> Signal {
    Signal::from_fn(|frame| (frame % 480) as f32 / 480.0)
}

/// Reference render for `loom.logic.compare`: a ramp against 0.5.
#[test]
fn compare() {
    Harness::new(&logic::COMPARE)
        .seconds(0.02)
        .input("a", ramp())
        .input("b", Signal::constant(0.5))
        .param("hysteresis", 0.1)
        .check(reference!("compare"));
}

/// Reference render for `loom.logic.logic`: Xor of two gates, inverted.
#[test]
fn logic_gates() {
    Harness::new(&logic::LOGIC)
        .seconds(0.02)
        .input("in1", Signal::gate(0.0, 0.01))
        .input("in2", Signal::gate(0.005, 0.015))
        .param("op", 2.0)
        .param("invert", 1.0)
        .check(reference!("logic"));
}

/// Reference render for `loom.logic.memory`: toggle mode.
#[test]
fn memory() {
    Harness::new(&logic::MEMORY)
        .seconds(0.04)
        .input("a", Signal::gate(0.005, 0.007))
        .input("b", Signal::gate(0.03, 0.032))
        .param("mode", 1.0)
        .check(reference!("memory"));
}

/// Reference render for `loom.rand.sample-hold`: a ramp held on a trigger.
#[test]
fn sample_hold() {
    Harness::new(&rand::SAMPLE_HOLD)
        .seconds(0.02)
        .input("in", ramp())
        .input(
            "trigger",
            Signal::from_fn(|frame| if frame % 200 == 0 { 1.0 } else { 0.0 }),
        )
        .check(reference!("sample_hold"));
}

/// Reference render for `loom.level.slew`: a square wave slowed down.
#[test]
fn slew() {
    Harness::new(&level::SLEW)
        .seconds(0.04)
        .input(
            "in",
            Signal::from_fn(|frame| if (frame / 480) % 2 == 0 { 1.0 } else { -1.0 }),
        )
        .param("rise", 0.005)
        .param("fall", 0.01)
        .check(reference!("slew"));
}

/// Reference render for `loom.osc.noise` in pink.
#[test]
fn noise_pink() {
    Harness::new(&osc::NOISE)
        .seconds(0.05)
        .param("seed", 7.0)
        .param("colour", 1.0)
        .check(reference!("noise_pink"));
}

/// Reference render for `loom.osc.noise` in brown.
#[test]
fn noise_brown() {
    Harness::new(&osc::NOISE)
        .seconds(0.05)
        .param("seed", 7.0)
        .param("colour", 2.0)
        .check(reference!("noise_brown"));
}

/// Reference render for `loom.phase.scale`: three against two on a 100 Hz phase.
#[test]
fn phase_scale() {
    Harness::new(&phase::SCALE)
        .seconds(0.05)
        .input("phase", Signal::phase(100.0))
        .param("multiply", 3.0)
        .param("divide", 2.0)
        .check(reference!("phase_scale"));
}

/// Reference render for `loom.mod.lfo`: a free triangle at 40 Hz.
#[test]
fn lfo_free() {
    Harness::new(&lfo::LFO)
        .seconds(0.05)
        .param("rate", 40.0)
        .param("shape", 1.0)
        .check(reference!("lfo_free"));
}

/// Reference render for `loom.mod.lfo`: a synced square, two waves per cycle.
#[test]
fn lfo_synced() {
    Harness::new(&lfo::LFO)
        .seconds(0.05)
        .input("phase", Signal::phase(50.0))
        .param("sync", 1.0)
        .param("cycles", 2.0)
        .param("shape", 4.0)
        .check(reference!("lfo_synced"));
}

/// Reference render for `loom.clock.clock`: dotted 1/16 at 300 BPM from bar
/// 1, with a gate width of 25 percent.
#[test]
fn clock() {
    Harness::new(&clock::CLOCK)
        .seconds(0.25)
        .transport(TransportSettings {
            tempo: 300.0,
            ..TransportSettings::default()
        })
        .param("rate", 19.0)
        .param("width", 25.0)
        .check(reference!("clock"));
}

/// Reference render for `loom.clock.transport`: its signals at 300 BPM in
/// 3/4 from bar 1. Without the engine nothing reads its requests, so the
/// render shows the outputs alone.
#[test]
fn transport() {
    Harness::new(&transport::TRANSPORT)
        .seconds(0.5)
        .transport(TransportSettings {
            tempo: 300.0,
            time_signature: loom_core::TimeSignature::new(3, 4),
            ..TransportSettings::default()
        })
        .check(reference!("transport"));
}
