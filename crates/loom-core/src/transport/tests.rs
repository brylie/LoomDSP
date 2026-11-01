use super::*;

const SAMPLE_RATE: f32 = 48_000.0;

#[test]
fn bar_phase_after_one_hour_is_within_one_sample() {
    // Acceptance: one hour at 120 BPM and 48 kHz.
    // The engine test `transport_stays_in_step_for_an_hour` runs the
    // full hour; this one checks the arithmetic alone.
    let transport = Transport::new(SAMPLE_RATE);
    let last = 3_600 * 48_000 - 1;
    let start = last - last % 1024;
    let measured = f64::from(transport.info(start).bar_phase_at((last - start) as usize));
    // 120 BPM in 4/4 at 48 kHz: one bar is exactly 96 000 samples.
    let samples_per_bar = 96_000u64;
    let exact = (last % samples_per_bar) as f64 / samples_per_bar as f64;
    let error = (measured - exact).abs().min(1.0 - (measured - exact).abs());
    assert!(error < 1.0 / samples_per_bar as f64, "error {error}");
}

#[test]
fn tempo_change_keeps_the_position() {
    let mut transport = Transport::new(SAMPLE_RATE);
    let at = 10_000;
    let before = transport.info(at).position();
    transport.set_tempo(at, 90.0);
    assert_eq!(transport.info(at).position(), before);
    let step = transport.info(at).position_at(1) - before;
    assert!((step - 90.0 / 60.0 / 48_000.0).abs() < 1e-15);
}

#[test]
fn stopping_holds_and_starting_continues() {
    let mut transport = Transport::new(SAMPLE_RATE);
    transport.set_running(4_800, false);
    let held = transport.info(4_800).position();
    assert!((held - 0.2).abs() < 1e-12);
    assert_eq!(transport.info(100_000).position_at(77), held);
    assert_eq!(transport.info(100_000).beats_per_sample(), 0.0);
    transport.set_running(100_000, true);
    assert_eq!(transport.info(100_000).position(), held);
    assert!(transport.info(100_001).position() > held);
}

#[test]
fn reset_jumps_for_one_block() {
    let mut transport = Transport::new(SAMPLE_RATE);
    transport.reset(5_000);
    let info = transport.info(5_000);
    assert_eq!(info.position(), 0.0);
    assert!(info.jumped());
    assert!(!info.later(1).jumped());
    transport.end_block();
    assert!(!transport.info(5_100).jumped());
}

/// A rate phase counts cycles from position 0, and a one-bar cycle is
/// the bar phase bit for bit.
#[test]
fn rate_phase_counts_from_bar_one() {
    let settings = TransportSettings {
        time_signature: TimeSignature::new(7, 8),
        ..TransportSettings::default()
    };
    let info = TransportInfo::new(SAMPLE_RATE, settings, 5.25);
    assert_eq!(info.phase_at(0, 1.0), 0.25);
    assert_eq!(info.phase_at(0, 2.0), 0.625);
    for frame in [0, 1, 77, 10_000] {
        assert_eq!(info.phase_at(frame, 3.5), info.bar_phase_at(frame));
        assert_eq!(info.phase_at(frame, 1.0), info.beat_phase_at(frame));
    }
}

/// Rule 10: a new time signature is a jump; the same one again is not.
/// A request from the Transport module applies the same way, and clamps
/// its tempo.
#[test]
fn a_time_signature_change_is_a_jump() {
    let mut transport = Transport::new(SAMPLE_RATE);
    transport.set_time_signature(TimeSignature::FOUR_FOUR);
    assert!(!transport.info(100).jumped());
    let before = transport.info(100).position();
    transport.set_time_signature(TimeSignature::new(3, 4));
    let info = transport.info(100);
    assert!(info.jumped());
    assert_eq!(info.position(), before, "the position does not move");
    transport.end_block();
    transport.apply(
        200,
        TransportRequest {
            tempo: Some(1_000.0),
            time_signature: Some(TimeSignature::new(7, 3)),
            running: Some(false),
            reset: false,
        },
    );
    let info = transport.info(200);
    assert_eq!(info.tempo(), MAX_TEMPO);
    assert_eq!(info.beats_per_bar(), 3.0, "an invalid time signature is ignored");
    assert!(!info.playing() && !info.jumped());
    transport.apply(
        300,
        TransportRequest {
            reset: true,
            ..TransportRequest::default()
        },
    );
    assert!(transport.info(300).jumped());
    assert_eq!(transport.info(300).position(), 0.0);
}

#[test]
fn phases_stay_below_one() {
    assert_eq!(phase(0.999_999_999_9), 0.0);
    assert_eq!(phase(3.0), 0.0);
    assert_eq!(phase(2.25), 0.25);
}

#[test]
fn time_signatures() {
    assert_eq!(TimeSignature::new(7, 8).beats_per_bar(), 3.5);
    assert_eq!(TimeSignature::new(6, 8).to_string(), "6/8");
    assert!(TimeSignature::new(32, 16).is_valid());
    assert!(!TimeSignature::new(0, 4).is_valid());
    assert!(!TimeSignature::new(33, 4).is_valid());
    assert!(!TimeSignature::new(7, 3).is_valid());
    assert!(!TimeSignature::new(4, 32).is_valid());
}

/// A numerator of 0 would give a bar length of 0 and NaN phases, so the
/// testing constructor refuses it.
#[test]
#[should_panic(expected = "invalid time signature 0/4")]
fn new_refuses_numerator_zero() {
    let settings = TransportSettings {
        time_signature: TimeSignature::new(0, 4),
        ..TransportSettings::default()
    };
    TransportInfo::new(48_000.0, settings, 0.0);
}

/// A denominator that is not a power of two up to 16 is refused too.
#[test]
#[should_panic(expected = "invalid time signature 7/3")]
fn new_refuses_denominator_three() {
    let settings = TransportSettings {
        time_signature: TimeSignature::new(7, 3),
        ..TransportSettings::default()
    };
    TransportInfo::new(48_000.0, settings, 0.0);
}

/// Every valid time signature gives finite phases.
#[test]
fn new_with_valid_signature_has_finite_phases() {
    let settings = TransportSettings {
        time_signature: TimeSignature::new(7, 8),
        ..TransportSettings::default()
    };
    let info = TransportInfo::new(48_000.0, settings, 1.0);
    assert!(info.bar_phase_at(10).is_finite());
}

#[test]
fn tempo_is_clamped() {
    assert_eq!(clamp_tempo(10.0), MIN_TEMPO);
    assert_eq!(clamp_tempo(1_000.0), MAX_TEMPO);
    assert_eq!(clamp_tempo(f64::NAN), DEFAULT_TEMPO);
    assert_eq!(clamp_tempo(96.0), 96.0);
}
