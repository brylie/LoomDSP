use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

/// Runs `descriptor` once over `inputs` (one slice per input port) with
/// the given parameters, in blocks of `block` frames, and returns output 0.
fn process(
    descriptor: &ModuleDescriptor,
    params: &[(usize, f32)],
    inputs: &[&[f32]],
    connected: &[bool],
    block: usize,
) -> Vec<f32> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!()
    };
    let frames = inputs[0].len();
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: block,
    });
    for &(index, value) in params {
        m.set_param(index, value);
    }
    m.reset();
    let mut out = vec![0.0; frames];
    for start in (0..frames).step_by(block) {
        let end = (start + block).min(frames);
        let slices: Vec<&[f32]> = inputs.iter().map(|s| &s[start..end]).collect();
        run(
            m.as_mut(),
            48_000.0,
            &slices,
            connected,
            &mut [&mut out[start..end]],
            &[],
        );
    }
    out
}

/// Without hysteresis, equal values are low in both modes.
#[test]
fn compare_modes_and_equality() {
    let a = [0.0, 0.5, 1.0];
    let b = [0.5; 3];
    assert_eq!(process(&COMPARE, &[], &[&a, &b], &[true, true], 3), [0.0, 0.0, 1.0]);
    assert_eq!(
        process(&COMPARE, &[(0, 1.0)], &[&a, &b], &[true, true], 3),
        [1.0, 0.0, 0.0]
    );
}

/// Noise smaller than the band does not make the gate chatter.
#[test]
fn compare_hysteresis_does_not_chatter() {
    // Rises past B = 0.5, then wobbles by +-0.05 around B.
    let mut a = vec![0.0, 0.8];
    a.extend((0..100).map(|i| 0.5 + if i % 2 == 0 { 0.05 } else { -0.05 }));
    let b = vec![0.5; a.len()];
    let gate = process(&COMPARE, &[(1, 0.2)], &[&a, &b], &[true, true], 64);
    let changes = gate.windows(2).filter(|w| w[0] != w[1]).count();
    assert_eq!(changes, 1, "only the first rise: {gate:?}");
    // With no hysteresis the same signal chatters.
    let gate = process(&COMPARE, &[], &[&a, &b], &[true, true], 64);
    assert!(gate.windows(2).filter(|w| w[0] != w[1]).count() > 10);
}

/// Logic ignores unconnected inputs, and invert flips the result.
#[test]
fn logic_counts_only_connected_inputs() {
    let high = [1.0; 2];
    let low = [0.0; 2];
    // And of two wires, with the other two inputs unconnected.
    let and = process(&LOGIC, &[], &[&high, &high, &low, &low], &[true, true, false, false], 2);
    assert_eq!(and, [1.0, 1.0]);
    // Or: one high is enough. Xor of two highs is low.
    let or = process(
        &LOGIC,
        &[(0, 1.0)],
        &[&high, &low, &low, &low],
        &[true, true, false, false],
        2,
    );
    assert_eq!(or, [1.0, 1.0]);
    let xor = process(
        &LOGIC,
        &[(0, 2.0)],
        &[&high, &high, &low, &low],
        &[true, true, false, false],
        2,
    );
    assert_eq!(xor, [0.0, 0.0]);
    // Xor of three highs (odd) is high.
    let xor3 = process(
        &LOGIC,
        &[(0, 2.0)],
        &[&high, &high, &high, &low],
        &[true, true, true, false],
        2,
    );
    assert_eq!(xor3, [1.0, 1.0]);
    // No cable: low, and Not (invert) makes it high.
    assert_eq!(process(&LOGIC, &[], &[&high[..]; 4], &[false; 4], 2), [0.0, 0.0]);
    assert_eq!(
        process(&LOGIC, &[(1, 1.0)], &[&high[..]; 4], &[false; 4], 2),
        [1.0, 1.0]
    );
    // Not: one connected input, Or, invert.
    assert_eq!(
        process(
            &LOGIC,
            &[(0, 1.0), (1, 1.0)],
            &[&high, &low, &low, &low],
            &[true, false, false, false],
            2
        ),
        [0.0, 0.0]
    );
}

/// Set/Reset: reset wins a tie, and edges fall on the same frame at
/// any block size, including the first and last frame of a block.
#[test]
fn memory_set_reset_and_block_invariance() {
    // a rises at 2 and 9, b rises at 5 and 9 (a tie).
    let mut a = vec![0.0; 16];
    let mut b = vec![0.0; 16];
    a[2..4].fill(1.0);
    a[9..11].fill(1.0);
    b[5..7].fill(1.0);
    b[9..11].fill(1.0);
    let expected: Vec<f32> = (0..16).map(|i| if (2..5).contains(&i) { 1.0 } else { 0.0 }).collect();
    for block in [1, 3, 4, 16] {
        assert_eq!(
            process(&MEMORY, &[], &[&a, &b], &[true, true], block),
            expected,
            "block {block}"
        );
    }
}

/// Toggle flips on each rising edge of A; B resets.
#[test]
fn memory_toggle_and_data() {
    let mut a = vec![0.0; 12];
    a[1..3].fill(1.0);
    a[5..7].fill(1.0);
    let mut b = vec![0.0; 12];
    b[8] = 1.0;
    let toggled = process(&MEMORY, &[(0, 1.0)], &[&a, &b], &[true, true], 12);
    let want: Vec<f32> = (0..12).map(|i| if (1..5).contains(&i) { 1.0 } else { 0.0 }).collect();
    assert_eq!(toggled, want);
    // Data: on a clock edge the output takes the level of A.
    let data = vec![0.0, 1.0, 1.0, 1.0, 0.0, 0.0];
    let clock = vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    assert_eq!(
        process(&MEMORY, &[(0, 2.0)], &[&data, &clock], &[true, true], 6),
        [0.0, 0.0, 1.0, 1.0, 1.0, 0.0]
    );
}
