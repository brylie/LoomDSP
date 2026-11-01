use super::*;

fn edges(detector: &mut EdgeDetector, input: &[f32]) -> Vec<bool> {
    input.iter().map(|&value| detector.rising(value)).collect()
}

#[test]
fn rising_edges_fire_once() {
    let input = [0.0, 1.0, 1.0, 0.0, 0.7, 0.5, 0.51];
    let found = edges(&mut EdgeDetector::new(), &input);
    assert_eq!(found, [false, true, false, false, true, false, true]);
}

#[test]
fn high_on_the_first_frame_fires() {
    assert_eq!(edges(&mut EdgeDetector::new(), &[1.0, 1.0]), [true, false]);
}

#[test]
fn one_sample_triggers_need_a_low_frame_between_them() {
    let separate = [trigger(true), trigger(false), trigger(true)];
    assert_eq!(edges(&mut EdgeDetector::new(), &separate), [true, false, true]);
    let merged = [trigger(true), trigger(true)];
    assert_eq!(edges(&mut EdgeDetector::new(), &merged), [true, false]);
}

#[test]
fn edges_do_not_depend_on_how_frames_are_split() {
    let input: Vec<f32> = (0..100).map(|i| if i % 7 < 3 { 1.0 } else { 0.0 }).collect();
    let whole = edges(&mut EdgeDetector::new(), &input);
    for block in [1, 2, 5, 64] {
        let mut detector = EdgeDetector::new();
        let split: Vec<bool> = input
            .chunks(block)
            .flat_map(|chunk| edges(&mut detector, chunk))
            .collect();
        assert_eq!(split, whole, "block size {block}");
    }
}

#[test]
fn reset_counts_as_low_again() {
    let mut detector = EdgeDetector::new();
    assert!(detector.rising(1.0));
    detector.reset();
    assert!(detector.rising(1.0));
}

#[test]
fn nan_is_never_high() {
    let mut detector = EdgeDetector::new();
    assert!(!detector.rising(f32::NAN));
    assert!(detector.rising(1.0));
}

#[test]
fn trigger_values() {
    assert_eq!(trigger(true), 1.0);
    assert_eq!(trigger(false), 0.0);
    assert!(gate_is_high(trigger(true)));
}

fn wraps(detector: &mut WrapDetector, input: &[f32]) -> Vec<bool> {
    input.iter().map(|&phase| detector.wrapped(phase)).collect()
}

#[test]
fn forward_wraps_fire_on_the_first_frame_of_the_new_cycle() {
    let input = [0.25, 0.5, 0.75, 0.0, 0.25, 0.5, 0.75, 0.0];
    let found = wraps(&mut WrapDetector::new(), &input);
    assert_eq!(found, [false, false, false, true, false, false, false, true]);
}

#[test]
fn backward_wraps_fire_too() {
    let input = [0.5, 0.25, 0.0, 0.75, 0.5];
    let found = wraps(&mut WrapDetector::new(), &input);
    assert_eq!(found, [false, false, false, true, false]);
}

#[test]
fn first_frame_fires_only_on_exactly_zero() {
    assert_eq!(wraps(&mut WrapDetector::new(), &[0.0, 0.1]), [true, false]);
    assert_eq!(wraps(&mut WrapDetector::new(), &[0.37, 0.4]), [false, false]);
    assert_eq!(wraps(&mut WrapDetector::new(), &[0.999, 0.0]), [false, true]);
}

#[test]
fn small_steps_and_wobble_do_not_fire() {
    let input = [0.4, 0.6, 0.55, 0.9, 0.45];
    let found = wraps(&mut WrapDetector::new(), &input);
    assert_eq!(found, [false; 5]);
}

#[test]
fn wraps_do_not_depend_on_how_frames_are_split() {
    let input: Vec<f32> = (0..200).map(|i| (i as f32 * 0.07).fract()).collect();
    let whole = wraps(&mut WrapDetector::new(), &input);
    for block in [1, 2, 5, 64] {
        let mut detector = WrapDetector::new();
        let split: Vec<bool> = input
            .chunks(block)
            .flat_map(|chunk| wraps(&mut detector, chunk))
            .collect();
        assert_eq!(split, whole, "block size {block}");
    }
}

#[test]
fn wrap_reset_forgets_the_previous_frame() {
    let mut detector = WrapDetector::new();
    assert!(detector.wrapped(0.0));
    assert!(!detector.wrapped(0.0));
    detector.reset();
    assert!(detector.wrapped(0.0));
}

#[test]
fn nan_never_wraps() {
    let mut detector = WrapDetector::new();
    assert!(!detector.wrapped(0.9));
    assert!(!detector.wrapped(f32::NAN));
    assert!(!detector.wrapped(0.1));
    assert!(detector.wrapped(0.95));
}
