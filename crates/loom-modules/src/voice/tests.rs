//! Unit tests for the voice modules' value conversions.

use super::Assign;

/// `unison` becomes a voice count from 1 to 4 for any value, including the
/// non-finite ones a direct `set_param` can send. The harness clamps a
/// parameter before `set_param`, so only a direct call reaches this.
#[test]
fn unison_becomes_a_count_from_one_to_four() {
    assert_eq!(Assign::unison_from(f32::NAN), 1);
    assert_eq!(Assign::unison_from(f32::NEG_INFINITY), 1);
    assert_eq!(Assign::unison_from(f32::INFINITY), 4);
    assert_eq!(Assign::unison_from(2.6), 3);
    assert_eq!(Assign::unison_from(0.0), 1);
}

/// `mode` becomes a stored choice from 0 to 3, and NaN gives the default,
/// Legato.
#[test]
fn mode_becomes_a_choice_with_nan_at_the_default() {
    assert_eq!(Assign::mode_from(f32::NAN), 2);
    assert_eq!(Assign::mode_from(f32::NEG_INFINITY), 0);
    assert_eq!(Assign::mode_from(f32::INFINITY), 3);
    assert_eq!(Assign::mode_from(1.4), 1);
}
