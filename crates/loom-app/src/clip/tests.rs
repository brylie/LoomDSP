use super::*;

const PROBE: ProbeId = ProbeId(1);

/// A mono reading.
fn reading(peak: f32, rms: f32) -> MeterReading {
    MeterReading {
        probe: PROBE,
        frame: 0,
        frames: 128,
        peak,
        rms,
        right: None,
    }
}

/// One step of the telemetry thread: readings in, and the news marked sent.
fn step(peaks: &mut Peaks, epoch: u64, readings: &[MeterReading]) -> Vec<(ProbeId, PeakNews)> {
    peaks.note(epoch, &HashSet::from([PROBE]), readings);
    let news = peaks.news();
    peaks.sent(&news);
    news
}

/// A steady peak is told once, so it does not wake the UI on every block.
#[test]
fn a_steady_peak_is_told_once() {
    let mut peaks = Peaks::default();
    assert_eq!(step(&mut peaks, 0, &[reading(0.5, 0.3)]).len(), 1);
    assert!(step(&mut peaks, 0, &[reading(0.5, 0.3)]).is_empty());
    assert!(
        step(&mut peaks, 0, &[reading(0.3, 0.2)]).is_empty(),
        "a lower peak is no news"
    );
    assert!(step(&mut peaks, 0, &[]).is_empty());
}

/// A peak is told once it rises by 0.1 dB, the readout's step, and not before.
#[test]
fn a_peak_is_told_when_the_readout_would_change() {
    let mut peaks = Peaks::default();
    step(&mut peaks, 0, &[reading(0.5, 0.3)]);
    assert!(
        step(&mut peaks, 0, &[reading(0.5 * 1.005, 0.3)]).is_empty(),
        "under 0.1 dB"
    );
    assert_eq!(step(&mut peaks, 0, &[reading(0.5 * 1.02, 0.3)]).len(), 1, "over 0.1 dB");
}

/// A peak that reaches 1.0 is told even when it rose by less than the step.
#[test]
fn reaching_the_clip_level_is_always_news() {
    let mut peaks = Peaks::default();
    step(&mut peaks, 0, &[reading(0.999, 0.3)]);
    let news = step(&mut peaks, 0, &[reading(1.0, 0.3)]);
    assert_eq!(news.len(), 1);
    assert!(news[0].1.clips());
}

/// A one-block clip is kept until the UI has it, though the signal has gone
/// and the first attempt to send it failed.
#[test]
fn a_clip_survives_a_busy_ui() {
    let mut peaks = Peaks::default();
    let meters = HashSet::from([PROBE]);
    peaks.note(0, &meters, &[reading(2.0, 0.5)]);
    // `try_send` failed: nothing marked sent.
    let _ = peaks.news();
    peaks.note(0, &meters, &[reading(0.0, 0.0)]);
    let news = peaks.news();
    assert_eq!(news[0].1.peak, 2.0);
    assert!(news[0].1.clips());
}

/// `f32::max` ignores NaN, so a NaN sample leaves the peak finite and only
/// the RMS shows it. Either one lights the light, and the readout never
/// holds a value that is not finite.
#[test]
fn a_value_that_is_not_finite_lights_the_light_and_not_the_readout() {
    for bad in [
        reading(0.2, f32::NAN),
        reading(f32::INFINITY, 0.1),
        reading(f32::NAN, f32::NAN),
    ] {
        let mut peaks = Peaks::default();
        let news = step(&mut peaks, 0, &[bad]);
        assert!(news[0].1.bad);
        let mut holds = ClipHolds::default();
        holds.take((1, 0), news[0].1);
        assert!(holds.get((1, 0)).lit);
        assert!(holds.get((1, 0)).peak.is_finite());
    }
}

/// A stereo meter uses the higher side.
#[test]
fn a_stereo_meter_uses_the_higher_side() {
    let mut peaks = Peaks::default();
    let news = step(
        &mut peaks,
        0,
        &[MeterReading {
            right: Some((1.5, 0.5)),
            ..reading(0.1, 0.1)
        }],
    );
    assert_eq!(news[0].1.peak, 1.5);
    assert!(news[0].1.clips());
}

/// After a reset, what still plays is news again, or a light cleared under a
/// hot signal would never come back.
#[test]
fn a_new_epoch_makes_everything_news_again() {
    let mut peaks = Peaks::default();
    step(&mut peaks, 0, &[reading(1.5, 0.5)]);
    assert!(step(&mut peaks, 0, &[reading(1.5, 0.5)]).is_empty());
    let news = step(&mut peaks, 1, &[reading(1.5, 0.5)]);
    assert!(news[0].1.clips());
    assert_eq!(peaks.epoch(), 1);
}

/// A click clears the light and keeps the readout. A double-click resets both.
#[test]
fn clearing_and_resetting() {
    let mut holds = ClipHolds::default();
    let meter = (3, 0);
    assert_eq!(holds.get(meter).text(), "--");
    assert!(holds.take(meter, PeakNews { peak: 2.0, bad: false }));
    assert!(!holds.take(meter, PeakNews { peak: 1.0, bad: false }), "nothing new");
    assert_eq!(holds.get(meter).text(), "+6.0 dB");
    holds.clear_light(meter);
    assert!(!holds.get(meter).lit);
    assert_eq!(holds.get(meter).text(), "+6.0 dB");
    holds.reset(meter);
    assert_eq!(holds.get(meter), Hold::default());
    assert_eq!(holds.epoch(), 2);
}
