use super::*;
use loom_core::testing::run;

/// Sample rate for these tests: 1 kHz, so one frame is 1 ms.
const RATE: f32 = 1_000.0;

/// An envelope at 1 kHz with its defaults and then `params` set.
fn make(params: &[(&str, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = ENVELOPE.role else {
        unreachable!()
    };
    let mut env = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: 4_000,
    });
    for (i, p) in ENVELOPE.params.iter().enumerate() {
        env.set_param(i, p.default);
    }
    for &(name, value) in params {
        let index = ENVELOPE.params.iter().position(|p| p.name == name).unwrap();
        env.set_param(index, value);
    }
    env.reset();
    env
}

/// Runs `env` over `gate` and returns its output.
fn play(env: &mut dyn Module, gate: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0; gate.len()];
    run(env, RATE, &[gate], &[true], &mut [&mut out], &[]);
    out
}

#[test]
fn follows_gate() {
    let mut env = make(&[]);
    let mut gate = vec![1.0; 2_000];
    gate[1_000..].fill(0.0);
    let out = play(env.as_mut(), &gate);
    assert!((out[999] - 0.7).abs() < 1e-6, "sustain level, got {}", out[999]);
    assert_eq!(out[1_999], 0.0, "released");
    assert!(out[5] > 0.0 && out[5] < 1.0, "attack ramps");
}

/// In ADSR mode with curve 0, every stage is a straight line that takes
/// its knob's time: 10 frames up, 20 down to sustain, 30 down to 0.
#[test]
fn adsr_stages_take_their_times() {
    let mut env = make(&[("attack", 0.01), ("decay", 0.02), ("sustain", 0.5), ("release", 0.03)]);
    let mut gate = vec![1.0; 100];
    gate[50..].fill(0.0);
    let out = play(env.as_mut(), &gate);
    // Frame 0 is the first step of the attack.
    assert!((out[0] - 0.1).abs() < 1e-6, "{}", out[0]);
    assert!((out[4] - 0.5).abs() < 1e-6, "{}", out[4]);
    assert_eq!(out[9], 1.0, "the attack ends after 10 frames");
    assert!((out[19] - 0.75).abs() < 1e-6, "half way down: {}", out[19]);
    assert_eq!(out[29], 0.5, "the decay ends after 20 frames");
    assert_eq!(out[49], 0.5, "sustain");
    assert!((out[64] - 0.25).abs() < 1e-6, "half way released: {}", out[64]);
    assert_eq!(out[79], 0.0, "the release ends after 30 frames");
}

/// AD mode measures its attack and decay times, ignores the gate going
/// low, and runs from a one-frame trigger.
#[test]
fn ad_mode_runs_once_from_a_trigger() {
    let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.04), ("sustain", 0.8)]);
    let mut gate = vec![0.0; 100];
    gate[0] = 1.0;
    let out = play(env.as_mut(), &gate);
    assert_eq!(out[9], 1.0, "the attack ends after 10 frames");
    assert!((out[29] - 0.5).abs() < 1e-6, "half way down: {}", out[29]);
    assert_eq!(out[49], 0.0, "the decay ends at 0, not at sustain");
    assert!(out[50..].iter().all(|&v| v == 0.0));

    // A held gate changes nothing: the decay still falls to 0.
    let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.04)]);
    let out = play(env.as_mut(), &[1.0; 100]);
    assert_eq!(out[49], 0.0);
    assert_eq!(out[99], 0.0);
}

/// A rising gate in the attack or in the decay starts a new attack from
/// the current level, so the output never jumps. The new attack takes
/// the full attack time.
#[test]
fn retrigger_starts_from_the_current_level() {
    for mode in [0.0, 1.0] {
        let mut env = make(&[("mode", mode), ("attack", 0.02), ("decay", 0.04), ("curve", 0.5)]);
        let mut gate = vec![0.0; 200];
        // In the attack at frame 10, and in the decay at frame 60.
        gate[0..5].fill(1.0);
        gate[10..15].fill(1.0);
        gate[60..65].fill(1.0);
        if mode == 0.0 {
            // ADSR: keep the gate high so the low edges do not release.
            gate[0..100].fill(1.0);
            gate[9] = 0.0;
            gate[59] = 0.0;
        }
        let out = play(env.as_mut(), &gate);
        let biggest = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        // A full attack of 20 frames moves at most about 1/20 per
        // frame at the start of a fast curve (k = 2.83).
        assert!(biggest < 0.2, "mode {mode}: step {biggest}");
        assert_eq!(out[79], 1.0, "mode {mode}: the retriggered attack takes 20 frames");
        assert!(out[78] < 1.0, "mode {mode}");
    }
}

/// Curve 0 is a straight line. Positive curve rises fast and falls fast
/// at first; negative curve starts slowly.
#[test]
fn curve_bends_the_stages() {
    let half = |curve: f32| {
        let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.01), ("curve", curve)]);
        let mut gate = vec![0.0; 30];
        gate[0] = 1.0;
        let out = play(env.as_mut(), &gate);
        (out[4], out[14])
    };
    let (rise, fall) = half(0.0);
    assert!((rise - 0.5).abs() < 1e-6 && (fall - 0.5).abs() < 1e-6);
    let (rise, fall) = half(1.0);
    // k = 8: 1 - 0.5^8 and 0.5^8.
    assert!((rise - (1.0 - 0.5f32.powi(8))).abs() < 1e-5, "{rise}");
    assert!((fall - 0.5f32.powi(8)).abs() < 1e-5, "{fall}");
    let (rise, fall) = half(-1.0);
    assert!(rise < 0.2 && fall > 0.8, "{rise} {fall}");
}

/// Switching to AD while the ADSR holds its sustain falls to 0 over
/// the decay time, without a jump.
#[test]
fn switching_to_ad_while_holding_decays() {
    let mut env = make(&[("attack", 0.01), ("decay", 0.01), ("sustain", 0.6)]);
    let out = play(env.as_mut(), &[1.0; 50]);
    assert_eq!(out[49], 0.6);
    env.set_param(4, 1.0);
    let out = play(env.as_mut(), &[1.0; 20]);
    assert!((out[0] - 0.54).abs() < 1e-6, "{}", out[0]);
    assert_eq!(out[9], 0.0);
}

/// Switching the mode partway through a decay restarts it from the
/// current level, so the output does not jump (#411 review). With the
/// gate high, ADSR falls on to the sustain level; with it low, ADSR
/// releases to 0.
#[test]
fn switching_mode_mid_decay_does_not_jump() {
    for (gate, after) in [(1.0, 0.5), (0.0, 0.0)] {
        let mut env = make(&[
            ("mode", 1.0),
            ("attack", 0.01),
            ("decay", 0.02),
            ("sustain", 0.5),
            ("release", 0.02),
        ]);
        let mut gates = [gate; 15];
        gates[0] = 1.0;
        let before = play(env.as_mut(), &gates);
        env.set_param(4, 0.0);
        let out = play(env.as_mut(), &[gate; 100]);
        let last = before[14];
        assert!(last > 0.5 && last < 1.0, "mid-decay, got {last}");
        assert!((out[0] - last).abs() < 0.06, "jump from {last} to {}", out[0]);
        let steps = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(steps < 0.06, "largest step {steps}");
        assert_eq!(out[99], after, "gate {gate}");
    }
    // From ADSR to AD mid-decay, the fall goes on to 0 without a jump.
    let mut env = make(&[("attack", 0.01), ("decay", 0.02), ("sustain", 0.5)]);
    let before = play(env.as_mut(), &[1.0; 15]);
    env.set_param(4, 1.0);
    let out = play(env.as_mut(), &[1.0; 100]);
    assert!(
        (out[0] - before[14]).abs() < 0.06,
        "jump from {} to {}",
        before[14],
        out[0]
    );
    assert_eq!(out[99], 0.0);
}

/// A trigger in AD mode, then a switch to ADSR during the attack, does
/// not hold the sustain with the gate low: the decay ends in the
/// release (#411 review).
#[test]
fn switching_to_adsr_after_a_trigger_releases() {
    let mut env = make(&[("mode", 1.0), ("attack", 0.01), ("decay", 0.01), ("release", 0.01)]);
    let mut gates = [0.0; 5];
    gates[0] = 1.0;
    play(env.as_mut(), &gates);
    env.set_param(4, 0.0);
    let out = play(env.as_mut(), &[0.0; 100]);
    assert_eq!(out[99], 0.0, "no stuck sustain");
}

/// The `level` display value is the output's last sample after the
/// attack, and 0 once the release has ended (#106).
#[test]
fn display_follows_the_level() {
    let mut env = make(&[]);
    let mut display = [f32::NAN];

    // The default attack is 10 ms, 10 frames at 1 kHz.
    let out = play(env.as_mut(), &[1.0; 50]);
    env.displays(&mut display);
    assert!(out[49] > 0.0, "past the attack, got {}", out[49]);
    assert_eq!(display[0], out[49]);

    let out = play(env.as_mut(), &[0.0; 2_000]);
    env.displays(&mut display);
    assert_eq!(out[1_999], 0.0, "released");
    assert_eq!(display[0], 0.0);
}
