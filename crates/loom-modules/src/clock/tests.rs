use super::*;
use loom_core::testing::run_with_transport;
use loom_core::{ModuleConfig, TimeSignature, TransportInfo, TransportSettings};

const SAMPLE_RATE: f32 = 48_000.0;
/// Frames per beat at 120 BPM and 48 kHz.
const BEAT: usize = 24_000;

/// A new Clock with its defaults and then `params` set.
fn clock(params: &[(usize, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = CLOCK.role else {
        unreachable!("the Clock is a DSP module")
    };
    let mut module = create(&ModuleConfig {
        sample_rate: SAMPLE_RATE,
        max_block: 1024,
    });
    for (index, param) in CLOCK.params.iter().enumerate() {
        module.set_param(index, param.default);
    }
    for &(index, value) in params {
        module.set_param(index, value);
    }
    module
}

/// Runs one call of `frames` frames and returns phase, trigger, gate.
fn call(module: &mut dyn Module, transport: TransportInfo, frames: usize) -> [Vec<f32>; 3] {
    let mut outputs = [vec![0.0; frames], vec![0.0; frames], vec![0.0; frames]];
    {
        let [a, b, c] = &mut outputs;
        let mut slices: Vec<&mut [f32]> = vec![a, b, c];
        run_with_transport(module, SAMPLE_RATE, transport, &[], &[], &mut slices, &[]);
    }
    outputs
}

/// Frames where `signal` is high.
fn highs(signal: &[f32]) -> Vec<usize> {
    signal
        .iter()
        .enumerate()
        .filter(|&(_, &x)| x > 0.5)
        .map(|(i, _)| i)
        .collect()
}

fn running(time_signature: TimeSignature) -> TransportSettings {
    TransportSettings {
        time_signature,
        ..TransportSettings::default()
    }
}

/// Rates in beats: bars follow the metre, notes do not.
#[test]
fn rates_in_beats() {
    assert_eq!(rate_beats(0, 4.0), 16.0);
    assert_eq!(rate_beats(6, 3.5), 3.5);
    assert_eq!(rate_beats(7, 4.0), 6.0);
    assert_eq!(rate_beats(QUARTER, 3.5), 1.0);
    assert_eq!(rate_beats(13, 4.0), 1.5);
    assert!((rate_beats(14, 4.0) - 2.0 / 3.0).abs() < 1e-15);
    assert_eq!(rate_beats(21, 4.0), 0.125);
    assert_eq!(RATE_NAMES.len(), 24);
    assert_eq!(RATE_NAMES[QUARTER], "1/4");
    assert_eq!(rate_index(f32::NAN), 0);
    assert_eq!(rate_index(99.0), 23);
}

/// A playing transport from bar 1 fires on frame 0 and on every beat,
/// and the gate is high for the first half of each beat.
#[test]
fn quarter_notes_fire_on_each_beat() {
    let mut module = clock(&[]);
    let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
    let [phase, trig, gate] = call(module.as_mut(), transport, 4 * BEAT + 1);
    assert_eq!(highs(&trig), [0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT]);
    assert_eq!(phase[BEAT / 2], 0.5);
    assert_eq!(gate[0], 1.0);
    assert_eq!(gate[BEAT / 2 - 1], 1.0);
    assert_eq!(gate[BEAT / 2], 0.0);
    assert_eq!(highs(&gate).len(), 4 * BEAT / 2 + 1);
}

/// The gate width sets the high part of each cycle.
#[test]
fn gate_width_sets_the_high_part() {
    let mut module = clock(&[(WIDTH, 25.0)]);
    let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
    let [_, _, gate] = call(module.as_mut(), transport, BEAT);
    assert_eq!(highs(&gate).len(), BEAT / 4);
}

/// A module added halfway through a cycle does not fire until the next
/// cycle starts.
#[test]
fn joining_mid_cycle_waits_for_the_next_cycle() {
    let mut module = clock(&[]);
    let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.5);
    let [_, trig, _] = call(module.as_mut(), transport, BEAT);
    assert_eq!(highs(&trig), [BEAT / 2]);
}

/// A stopped transport fires nothing and holds the phase, even across a
/// reset. Starting from bar 1 fires once, on the first frame.
#[test]
fn stopped_transport_holds_and_start_from_bar_one_fires_once() {
    let mut module = clock(&[]);
    let stopped = TransportSettings {
        running: false,
        ..TransportSettings::default()
    };
    let held = TransportInfo::new(SAMPLE_RATE, stopped, 2.25);
    let [phase, trig, _] = call(module.as_mut(), held, 256);
    assert!(phase.iter().all(|&p| p == 0.25));
    assert!(highs(&trig).is_empty());
    // A reset while stopped jumps to 0, but nothing fires yet.
    let reset = TransportInfo::new(SAMPLE_RATE, stopped, 0.0).with_jump(true);
    let [phase, trig, _] = call(module.as_mut(), reset, 256);
    assert!(phase.iter().all(|&p| p == 0.0));
    assert!(highs(&trig).is_empty());
    // Start: bar 1 fires on its first frame.
    let start = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
    let [_, trig, _] = call(module.as_mut(), start, 256);
    assert_eq!(highs(&trig), [0]);
}

/// Starting again where a stop left off, mid-cycle, fires no trigger.
#[test]
fn continuing_mid_cycle_does_not_fire() {
    let mut module = clock(&[]);
    let stopped = TransportSettings {
        running: false,
        ..TransportSettings::default()
    };
    call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, stopped, 1.3), 64);
    let start = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 1.3);
    let [_, trig, _] = call(module.as_mut(), start, 64);
    assert!(highs(&trig).is_empty());
}

/// A reset while playing fires once on bar 1, however far the position
/// jumped (rule 8).
#[test]
fn reset_while_playing_fires_once() {
    let mut module = clock(&[]);
    let settings = running(TimeSignature::FOUR_FOUR);
    call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, settings, 0.3), 64);
    let reset = TransportInfo::new(SAMPLE_RATE, settings, 0.0).with_jump(true);
    let [_, trig, _] = call(module.as_mut(), reset, 64);
    assert_eq!(highs(&trig), [0]);
}

/// A jump that lands mid-cycle, such as a new time signature under a bar
/// rate, fires nothing, even when the phase moves by more than half.
#[test]
fn a_jump_mid_cycle_does_not_fire() {
    let mut module = clock(&[(RATE, 6.0)]);
    call(
        module.as_mut(),
        TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 1.0),
        64,
    );
    // At position 1 the bar phase is 0.25 in 4/4 and 0.8 in 5/16 (1.25 beats a bar).
    let jumped = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::new(5, 16)), 1.0).with_jump(true);
    let [phase, trig, _] = call(module.as_mut(), jumped, 64);
    assert!((phase[0] - 0.8).abs() < 1e-6);
    assert!(highs(&trig).is_empty());
}

/// Turning the rate moves the phase at once but fires no trigger.
#[test]
fn a_rate_change_does_not_fire() {
    let mut module = clock(&[]);
    let settings = running(TimeSignature::FOUR_FOUR);
    call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, settings, 0.9), 64);
    // 1/4 at 0.9 beats is phase 0.9; 1/2 is phase 0.45: a jump of more than half.
    module.set_param(RATE, 9.0);
    let [phase, trig, _] = call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, settings, 0.9), 64);
    assert!((phase[0] - 0.45).abs() < 1e-6);
    assert!(highs(&trig).is_empty());
}

/// A triplet quarter wraps three times in two beats, and is back on the
/// beat grid at bar 1 of each two beats.
#[test]
fn triplets_fire_three_times_in_two_beats() {
    let mut module = clock(&[(RATE, 14.0)]);
    let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
    let [_, trig, _] = call(module.as_mut(), transport, 2 * BEAT + 1);
    assert_eq!(highs(&trig), [0, 16_000, 32_000, 48_000]);
}
