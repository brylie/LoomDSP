use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

/// Runs the module over the inputs in blocks of `block` frames.
fn process(mode: f32, input: &[f32], trigger: &[f32], block: usize) -> Vec<f32> {
    let ModuleRole::Dsp(create) = SAMPLE_HOLD.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: block,
    });
    m.set_param(0, mode);
    m.reset();
    let mut out = vec![0.0; input.len()];
    for start in (0..input.len()).step_by(block) {
        let end = (start + block).min(input.len());
        run(
            m.as_mut(),
            48_000.0,
            &[&input[start..end], &trigger[start..end]],
            &[true, true],
            &mut [&mut out[start..end]],
            &[],
        );
    }
    out
}

/// Sample takes the input on the frame of the rising edge, at any block
/// size; Track follows the input while the trigger is high.
#[test]
fn samples_on_the_edge_and_tracks_while_high() {
    let input: Vec<f32> = (0..12).map(|i| i as f32).collect();
    let mut trigger = vec![0.0; 12];
    trigger[3..6].fill(1.0);
    trigger[9] = 1.0;
    let sampled = [0.0, 0.0, 0.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 9.0, 9.0, 9.0];
    let tracked = [0.0, 0.0, 0.0, 3.0, 4.0, 5.0, 5.0, 5.0, 5.0, 9.0, 9.0, 9.0];
    for block in [1, 5, 12] {
        assert_eq!(process(0.0, &input, &trigger, block), sampled, "sample, block {block}");
        assert_eq!(process(1.0, &input, &trigger, block), tracked, "track, block {block}");
    }
}

/// Voice bits never collide with stream number or stereo side, and voice 1 is unchanged.
#[test]
fn voice_streams_have_distinct_identity_bits() {
    assert_eq!(stream(42, 0, 0), Rng::new(42));
    assert_eq!(stream(42, 1, 3), Rng::new(42 | 1 << 16 | 3 << 17));
    assert_ne!(stream(42, 2, 0), stream(42, 0, 1));
    let mut streams = Vec::new();
    for voice in 0..16 {
        for side in 0..2 {
            for number in 0..4 {
                let next = stream(42, voice * 2 + side, number);
                assert!(!streams.contains(&next));
                streams.push(next);
            }
        }
    }
}
