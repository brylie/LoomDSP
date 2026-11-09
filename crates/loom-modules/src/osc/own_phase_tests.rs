//! The oscillators' own phase (`docs/design/oscillators.md`, rules 1 to 5).

use super::*;
use crate::phase::PHASE;
use loom_core::signal::pitch;
use loom_core::testing::run;

const RATE: f32 = 48_000.0;
const FRAMES: usize = 2_000;

/// The four oscillators, which all run their own phase.
const OSCILLATORS: [&ModuleDescriptor; 4] = [&SINE, &SAW, &PULSE, &TRIANGLE];

/// A new instance of `descriptor`, reset as the controller does.
fn create(descriptor: &ModuleDescriptor, params: &[(&str, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!("oscillators are DSP modules")
    };
    let mut module = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: FRAMES,
    });
    for &(name, value) in params {
        module.set_param(descriptor.param_index(name).expect("known parameter"), value);
    }
    module.reset();
    module
}

/// A pitch that glides from a fifth below middle C to an octave above it.
fn gliding_pitch() -> Vec<f32> {
    (0..FRAMES)
        .map(|i| -7.0 / 120.0 + 0.15 * i as f32 / FRAMES as f32)
        .collect()
}

/// Renders an oscillator with its phase input unconnected.
fn own(descriptor: &ModuleDescriptor, params: &[(&str, f32)], pitch: &[f32]) -> Vec<f32> {
    let mut module = create(descriptor, params);
    let mut out = vec![0.0; pitch.len()];
    let zeros = vec![0.0; pitch.len()];
    run(
        module.as_mut(),
        RATE,
        &[pitch, &zeros],
        &[true, false],
        &mut [&mut out],
        &[],
    );
    out
}

/// Renders an oscillator with `phase` on a cable into its phase input.
fn cabled(descriptor: &ModuleDescriptor, params: &[(&str, f32)], pitch: &[f32], phase: &[f32]) -> Vec<f32> {
    let mut module = create(descriptor, params);
    let mut out = vec![0.0; phase.len()];
    run(
        module.as_mut(),
        RATE,
        &[pitch, phase],
        &[true, true],
        &mut [&mut out],
        &[],
    );
    out
}

/// What a Phase module with these knobs makes from `pitch`.
fn phase_module(tune: f32, octave: f32, pitch: &[f32]) -> Vec<f32> {
    let mut module = create(&PHASE, &[("tune", tune), ("octave", octave)]);
    let mut out = vec![0.0; pitch.len()];
    let reset = vec![0.0; pitch.len()];
    run(
        module.as_mut(),
        RATE,
        &[pitch, &reset],
        &[true, false],
        &mut [&mut out],
        &[],
    );
    out
}

/// Rule 1: with no phase cable, an oscillator plays at its pitch and knobs,
/// exactly as a Phase module with the same knobs in front of it would. So
/// removing a Phase module that only fed one oscillator changes nothing.
#[test]
fn own_phase_matches_a_phase_module_in_front() {
    let pitch = gliding_pitch();
    let phase = phase_module(7.0, -1.0, &pitch);
    for descriptor in OSCILLATORS {
        let tuned = [("tune", 7.0), ("octave", -1.0)];
        let own = own(descriptor, &tuned, &pitch);
        let through_phase = cabled(descriptor, &[], &pitch, &phase);
        assert_eq!(own, through_phase, "{}", descriptor.id);
        assert!(own.iter().any(|v| v.abs() > 0.5), "{} makes a sound", descriptor.id);
    }
}

/// Rule 1: an oscillator with nothing connected plays middle C.
#[test]
fn unconnected_oscillator_plays_middle_c() {
    let seconds = 1.0;
    let frames = (RATE * seconds) as usize;
    let out = own(&SAW, &[], &vec![0.0; frames]);
    // The saw rises through 0 once per cycle, halfway up its ramp. Its fall
    // is smoothed over two samples, so it is not counted.
    let rises = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32;
    let hz = rises / seconds;
    assert!((hz - pitch::REFERENCE_HZ).abs() <= 1.0, "{hz} Hz");
}

/// Rule 2: a phase cable decides the wave. Pitch, tune, and octave do nothing.
#[test]
fn phase_cable_ignores_pitch_and_knobs() {
    let phase: Vec<f32> = (0..FRAMES).map(|i| (i as f32 * 0.003) % 1.0).collect();
    for descriptor in OSCILLATORS {
        let plain = cabled(descriptor, &[], &vec![0.0; FRAMES], &phase);
        let tuned = cabled(descriptor, &[("tune", 12.0), ("octave", 3.0)], &gliding_pitch(), &phase);
        assert_eq!(plain, tuned, "{}", descriptor.id);
    }
}

/// Rule 3: the own phase holds while a cable is connected, and continues
/// from where it was when the cable is removed.
#[test]
fn own_phase_holds_while_a_cable_is_connected() {
    let pitch = vec![0.0; 300];
    let zeros = vec![0.0; 100];
    let cable = vec![0.25; 100];
    let mut module = create(&SAW, &[]);
    let mut first = vec![0.0; 100];
    let mut held = vec![0.0; 100];
    let mut after = vec![0.0; 100];
    run(
        module.as_mut(),
        RATE,
        &[&pitch[..100], &zeros],
        &[true, false],
        &mut [&mut first],
        &[],
    );
    run(
        module.as_mut(),
        RATE,
        &[&pitch[..100], &cable],
        &[true, true],
        &mut [&mut held],
        &[],
    );
    run(
        module.as_mut(),
        RATE,
        &[&pitch[..100], &zeros],
        &[true, false],
        &mut [&mut after],
        &[],
    );

    let mut unbroken = own(&SAW, &[], &pitch[..200]);
    let resumed: Vec<f32> = first.iter().chain(&after).copied().collect();
    // The band limit reads the last phase, so the first frame after the
    // cable differs. Every frame after it is the unbroken wave.
    unbroken.drain(100..101);
    assert_eq!(resumed[..100], unbroken[..100]);
    assert_eq!(resumed[101..], unbroken[100..]);
}

/// Rule 4: the octave knob moves in whole octaves.
#[test]
fn octave_rounds_to_whole_octaves() {
    let pitch = vec![0.0; FRAMES];
    assert_eq!(
        own(&SINE, &[("octave", 1.4)], &pitch),
        own(&SINE, &[("octave", 1.0)], &pitch)
    );
}

/// Rule 5: a pitch far too high, or not a number, does not stop the
/// oscillator. It plays again when the pitch comes back.
#[test]
fn oscillator_recovers_from_a_wild_pitch() {
    let mut pitch = vec![0.0; FRAMES];
    pitch[100..200].fill(50.0);
    pitch[200..300].fill(f32::NAN);
    for descriptor in OSCILLATORS {
        let out = own(descriptor, &[], &pitch);
        assert!(out.iter().all(|v| v.is_finite()), "{}", descriptor.id);
        assert!(
            out[1_000..].iter().any(|v| v.abs() > 0.5),
            "{} plays again",
            descriptor.id
        );
    }
}

/// Rule 6: a reset starts the own phase again from 0, so a render repeats.
#[test]
fn reset_starts_the_own_phase_again() {
    let pitch = gliding_pitch();
    for descriptor in OSCILLATORS {
        let mut module = create(descriptor, &[]);
        let zeros = vec![0.0; FRAMES];
        let mut first = vec![0.0; FRAMES];
        let mut second = vec![0.0; FRAMES];
        run(
            module.as_mut(),
            RATE,
            &[&pitch, &zeros],
            &[true, false],
            &mut [&mut first],
            &[],
        );
        module.reset();
        run(
            module.as_mut(),
            RATE,
            &[&pitch, &zeros],
            &[true, false],
            &mut [&mut second],
            &[],
        );
        assert_eq!(first, second, "{}", descriptor.id);
    }
}
