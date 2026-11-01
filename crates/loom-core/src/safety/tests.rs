use super::*;

const RATE: f32 = 48_000.0;

fn run(stage: &mut SafetyStage, input: &[f32]) -> Vec<f32> {
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    stage.process(&mut left, &mut right);
    assert_eq!(left, right);
    left
}

#[test]
fn modes_read_back_from_their_names() {
    for mode in OutputSafety::ALL {
        assert_eq!(mode.name().parse(), Ok(mode));
    }
    assert!("loud".parse::<OutputSafety>().is_err());
}

#[test]
fn off_only_sanitizes_and_clips_after_the_fade() {
    let mut stage = SafetyStage::new(OutputSafety::Off, RATE);
    run(&mut stage, &vec![0.0; 1000]);
    let input = [0.25, -0.5, 1.5, -7.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e38];
    assert_eq!(run(&mut stage, &input), [0.25, -0.5, 1.0, -1.0, 0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn the_fade_starts_at_zero_and_rises_in_a_straight_line() {
    let mut stage = SafetyStage::new(OutputSafety::Off, RATE);
    let out = run(&mut stage, &vec![0.5; 1000]);
    let length = samples(FADE_SECONDS, RATE);
    assert_eq!(out[0], 0.0);
    assert!((out[length / 2] - 0.25).abs() < 1e-6);
    assert_eq!(out[length], 0.5);
    assert!(out.windows(2).all(|w| w[1] >= w[0]));
}

#[test]
fn clean_mode_ramps_the_gain_down_before_a_peak() {
    let mut limiter = Limiter::new(RATE);
    let lookahead = limiter.lookahead;
    let input: Vec<f32> = (0..400).map(|i| if i < 200 { 0.5 } else { 2.0 }).collect();
    let out: Vec<f32> = input.iter().map(|&x| limiter.tick(x, x, true).0).collect();
    assert!(out[..lookahead].iter().all(|&s| s == 0.0));
    assert!(out.iter().all(|s| s.abs() <= CEILING));
    // The quiet samples just before the step come out turned down a
    // little more each time: a ramp, not a step.
    let ramp = &out[199..200 + lookahead];
    assert_eq!(ramp[0], 0.5);
    assert!(ramp.windows(2).all(|w| w[1] < w[0]));
    assert!(ramp[lookahead] < 0.25);
    // The loud sample comes out at the ceiling.
    assert!((out[200 + lookahead] - CEILING).abs() < 1e-5);
}

#[test]
fn low_latency_mode_turns_down_the_sample_that_goes_over() {
    let mut limiter = Limiter::new(RATE);
    assert_eq!(limiter.tick(0.5, -0.25, false), (0.5, -0.25));
    assert_eq!(limiter.tick(2.0, -1.0, false), (CEILING, -CEILING / 2.0));
    // Then it recovers slowly.
    let (next, _) = limiter.tick(0.5, 0.0, false);
    assert!(next < 0.25);
}

#[test]
fn a_large_held_offset_decays_to_zero() {
    for sample_rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let mut dc = DcBlocker::new(sample_rate);
        let mut output = 0.0;
        for _ in 0..sample_rate as usize {
            output = dc.tick(INPUT_LIMIT);
        }
        // About e^-31 of the step after 1 s. Rounding once held it near
        // 0.0015 at 48 kHz.
        assert!(output.abs() < 1e-9, "{sample_rate} Hz: {output}");
    }
}

#[test]
fn huge_values_do_not_stick() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean] {
        let mut stage = SafetyStage::new(mode, RATE);
        run(&mut stage, &[1e38, f32::INFINITY, f32::NAN, 1e38, -1e38]);
        let sine: Vec<f32> = (0..48_000)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / RATE).sin())
            .collect();
        let out = run(&mut stage, &sine);
        let peak = out[24_000..].iter().fold(0.0f32, |p, s| p.max(s.abs()));
        assert!(peak > 0.49 && peak <= 0.5, "{mode:?}: {peak}");
    }
}

#[test]
fn a_mode_change_dips_to_silence_and_back() {
    let mut stage = SafetyStage::new(OutputSafety::LowLatency, RATE);
    let length = samples(FADE_SECONDS, RATE);
    run(&mut stage, &vec![0.5; 2 * length]);
    stage.set_mode(OutputSafety::Clean);
    // Asking again for the mode on its way changes nothing.
    stage.set_mode(OutputSafety::Clean);
    let out = run(&mut stage, &vec![0.5; 4 * length]);
    let silent = out.iter().position(|&s| s == 0.0).unwrap();
    assert!((length..length + 2).contains(&silent), "{silent}");
    assert_eq!(stage.mode, OutputSafety::Clean);
    assert!(stage.pending.is_none());
    // No jump bigger than one fade step, plus the DC blocker's drift.
    let step = 0.5 / length as f32 + 1e-3;
    assert!(out.windows(2).all(|w| (w[1] - w[0]).abs() <= step));
}

#[test]
fn a_fade_out_during_a_dip_goes_down_and_stays_down() {
    let mut stage = SafetyStage::new(OutputSafety::Off, RATE);
    let length = samples(FADE_SECONDS, RATE);
    run(&mut stage, &vec![0.5; 2 * length]);
    stage.set_mode(OutputSafety::Clean);
    run(&mut stage, &vec![0.5; length / 2]);
    stage.fade_out();
    // A mode asked for while stopping changes nothing.
    stage.set_mode(OutputSafety::LowLatency);
    let out = run(&mut stage, &vec![0.5; 2 * length]);
    assert!(out.windows(2).all(|w| w[1] <= w[0]), "only goes down");
    let silent = out.iter().position(|&s| s == 0.0).unwrap();
    assert!(silent <= length / 2 + 1, "{silent}");
    assert!(out[silent..].iter().all(|&s| s == 0.0), "and stays at 0");
    assert!(stage.is_silent());
    assert_eq!(stage.mode, OutputSafety::Off);
}

#[test]
fn a_fade_out_while_clean_waits_for_its_first_sample_is_silent_at_once() {
    let mut stage = SafetyStage::new(OutputSafety::Clean, RATE);
    stage.fade_out();
    assert!(!stage.is_silent(), "nothing played yet");
    let out = run(&mut stage, &[0.5; 4]);
    assert_eq!(out, [0.0; 4]);
    assert!(stage.is_silent());
}
