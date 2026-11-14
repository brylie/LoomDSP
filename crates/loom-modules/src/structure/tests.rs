use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

const RATE: f32 = 48_000.0;

/// A swap fade at 48 kHz.
fn fade() -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = FADE.role else {
        unreachable!()
    };
    create(&ModuleConfig {
        sample_rate: RATE,
        max_block: 4_800,
    })
}

/// The fade's kind is the one `loom-patch` adds during a swap.
#[test]
fn the_fade_kind_matches_the_patch_crate() {
    assert_eq!(FADE.kind(), loom_patch::slot::FADE_KIND);
    assert_eq!((SWAP_FADE_SECONDS * RATE).round() as usize, 480);
}

/// Over 10 ms the fade moves from A alone to B alone on a quarter circle,
/// so the squared gains sum to 1 at every frame, and then it outputs B
/// exactly and reports done (sub-patches.md, How a swap crossfades).
#[test]
fn the_fade_is_equal_power_and_ends_on_b_exactly() {
    let mut fade = fade();
    let frames = 600;
    let a = vec![1.0; frames];
    let b = vec![-0.5; frames];
    let mut out = vec![0.0; frames];
    let mut displays = [0.0];
    fade.displays(&mut displays);
    assert_eq!(displays[0], 0.0, "not done before the first frame");
    run(fade.as_mut(), RATE, &[&a, &b], &[true, true], &mut [&mut out], &[]);
    assert_eq!(out[0], 1.0, "the first frame is A alone");
    for (i, value) in out.iter().enumerate().take(480) {
        let theta = std::f32::consts::FRAC_PI_2 * i as f32 / 480.0;
        let expected = theta.cos() - 0.5 * theta.sin();
        assert!((value - expected).abs() < 1e-5, "frame {i}: {value} vs {expected}");
        let (gain_b, gain_a) = theta.sin_cos();
        assert!((gain_a * gain_a + gain_b * gain_b - 1.0).abs() < 1e-5);
    }
    assert!(out[480..].iter().all(|v| *v == -0.5), "B exactly once done");
    fade.displays(&mut displays);
    assert_eq!(displays[0], 1.0);
}

/// The fade counts per sample, so two short calls give the same output as
/// one long one (block-size invariance), and `reset` starts it again.
#[test]
fn the_fade_is_block_size_invariant() {
    let a = vec![0.8; 500];
    let b = vec![0.2; 500];
    let mut whole = vec![0.0; 500];
    let mut one = fade();
    run(one.as_mut(), RATE, &[&a, &b], &[true, true], &mut [&mut whole], &[]);
    let mut parts = vec![0.0; 500];
    let mut two = fade();
    let (first, second) = parts.split_at_mut(123);
    run(
        two.as_mut(),
        RATE,
        &[&a[..123], &b[..123]],
        &[true, true],
        &mut [first],
        &[],
    );
    run(
        two.as_mut(),
        RATE,
        &[&a[123..], &b[123..]],
        &[true, true],
        &mut [second],
        &[],
    );
    assert_eq!(whole, parts);
    two.reset();
    let mut again = vec![0.0; 1];
    run(
        two.as_mut(),
        RATE,
        &[&a[..1], &b[..1]],
        &[true, true],
        &mut [&mut again],
        &[],
    );
    assert_eq!(again[0], 0.8, "after reset the fade starts from A");
}

/// The swap fade's kind names the module ID that live patching rule 9
/// treats as a mixer, so a rename cannot leave the two apart.
#[test]
fn the_fade_kind_names_the_core_mixer_id() {
    assert_eq!(FADE_KIND, format!("{}@1", loom_core::cable_fade::SWAP_FADE_MODULE));
}

/// Every module rule 9 treats as a mixer is registered, so the core's list
/// cannot name a module that does not exist.
#[test]
fn every_mixer_is_registered() {
    let registry = crate::registry();
    for id in loom_core::cable_fade::MIXERS
        .iter()
        .chain([&loom_core::cable_fade::MEMBER_MODULE])
    {
        assert!(registry.get(id).is_some(), "{id}");
    }
}
