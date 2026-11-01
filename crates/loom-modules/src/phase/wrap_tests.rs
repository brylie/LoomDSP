use super::*;

/// Wrap and fold give the values in the spec for the edges and for
/// negative input, and 0 for NaN and infinity.
#[test]
fn wrap_and_fold_values() {
    let inputs = [-0.25, 0.0, 0.25, 1.0, 1.25, 2.0];
    let wrapped = [0.75, 0.0, 0.25, 0.0, 0.25, 0.0];
    let folded = [0.25, 0.0, 0.25, 1.0, 0.75, 0.0];
    for ((x, w), f) in inputs.iter().zip(wrapped).zip(folded) {
        assert_eq!(Wrap::wrap(*x), w, "wrap {x}");
        assert_eq!(Wrap::fold(*x), f, "fold {x}");
    }
    // A tiny negative number must not give exactly 1.
    assert!(Wrap::wrap(-1e-9) < 1.0);
}

/// NaN and infinity give 0 in both modes, through the module.
#[test]
fn non_finite_input_gives_zero() {
    use loom_core::testing::run;
    let input = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.25];
    for fold in [0.0, 1.0] {
        let ModuleRole::Dsp(create) = WRAP.role else {
            unreachable!()
        };
        let mut m = create(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 4,
        });
        m.set_param(0, fold);
        let mut out = [9.0; 4];
        run(m.as_mut(), 48_000.0, &[&input], &[true], &mut [&mut out], &[]);
        assert_eq!(out[..3], [0.0, 0.0, 0.0]);
        assert!(out[3] > 0.0 && out[3] <= 1.0);
    }
}
