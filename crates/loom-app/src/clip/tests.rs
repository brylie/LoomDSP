use super::*;

const PROBE: ProbeId = ProbeId(1);

/// The one meter most tests watch, on [`PROBE`].
const METER: MeterWidget = (1, 0);

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

/// One step of the telemetry thread for [`METER`]: readings in, and the news
/// marked sent.
fn step(peaks: &mut Peaks, resets: u64, readings: &[MeterReading]) -> Vec<(MeterWidget, PeakNews)> {
    peaks.note(&HashMap::from([(METER, (PROBE, resets))]), readings);
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
    let meters = HashMap::from([(METER, (PROBE, 0))]);
    peaks.note(&meters, &[reading(2.0, 0.5)]);
    // `try_send` failed: nothing marked sent.
    let _ = peaks.news();
    peaks.note(&meters, &[reading(0.0, 0.0)]);
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
fn a_new_reset_count_makes_it_news_again() {
    let mut peaks = Peaks::default();
    step(&mut peaks, 0, &[reading(1.5, 0.5)]);
    assert!(step(&mut peaks, 0, &[reading(1.5, 0.5)]).is_empty());
    let news = step(&mut peaks, 1, &[reading(1.5, 0.5)]);
    assert!(news[0].1.clips());
    assert_eq!(news[0].1.after, 1, "news says which reset it came after");
    assert_eq!(news[0].1.probe, Some(PROBE), "and which probe it came from");
}

/// A click clears the light and keeps the readout. A double-click resets both.
#[test]
fn clearing_and_resetting() {
    let mut holds = ClipHolds::default();
    let meter = (3, 0);
    assert_eq!(holds.get(meter).text(), "--");
    let news = |peak| PeakNews {
        peak,
        ..PeakNews::default()
    };
    assert!(holds.take(meter, news(2.0)));
    assert!(!holds.take(meter, news(1.0)), "nothing new");
    assert_eq!(holds.get(meter).text(), "+6.0 dB");
    holds.clear_light(meter);
    assert!(!holds.get(meter).lit);
    assert_eq!(holds.get(meter).text(), "+6.0 dB");
    holds.reset(meter);
    assert_eq!(holds.get(meter), Hold::default());
    assert_eq!(holds.resets(meter), 2);
    assert_eq!(holds.resets((4, 0)), 0, "each meter counts its own");
}

/// A reset of one meter leaves another probe's unsent clip alone, though the
/// signal has moved on and nothing would report it again.
#[test]
fn a_reset_of_one_meter_keeps_a_clip_on_another() {
    let other = ProbeId(2);
    let second: MeterWidget = (2, 0);
    let meters = |first_resets| HashMap::from([(METER, (PROBE, first_resets)), (second, (other, 0))]);
    let mut peaks = Peaks::default();
    let on_other = MeterReading {
        probe: other,
        ..reading(2.0, 0.5)
    };
    peaks.note(&meters(0), &[reading(1.5, 0.5), on_other]);
    // The UI resets the first meter before the news reaches it.
    peaks.note(&meters(1), &[]);
    let news = peaks.news();
    assert_eq!(news.len(), 1, "the first meter starts again");
    assert_eq!(news[0].0, second);
    assert_eq!(news[0].1.peak, 2.0);
    assert_eq!(news[0].1.after, 0, "the second meter was never reset");
}

/// Two meters on one probe: each reading reaches both, a reset of one clears
/// only its news, and the other's news keeps its own count
/// (`docs/design/panels.md`, Meter widget, Two meters on one output).
#[test]
fn two_meters_on_one_probe_keep_their_own_news() {
    let (left, right): (MeterWidget, MeterWidget) = ((1, 0), (1, 1));
    let meters = |left_resets| HashMap::from([(left, (PROBE, left_resets)), (right, (PROBE, 0))]);
    let mut peaks = Peaks::default();
    peaks.note(&meters(0), &[reading(2.0, 0.5)]);
    let mut news = peaks.news();
    news.sort_by_key(|(meter, _)| *meter);
    assert_eq!(news.len(), 2, "the reading reaches both meters");
    assert!(news.iter().all(|(_, n)| n.clips() && n.after == 0));

    // The UI resets the left meter while that news is on its way.
    peaks.note(&meters(1), &[]);
    let news = peaks.news();
    assert_eq!(news.len(), 1, "only the left meter starts again");
    assert_eq!(news[0].0, right);
    assert!(news[0].1.clips());
    assert_eq!(news[0].1.after, 0, "the right meter keeps its count");

    // What plays after the reset is news for the left meter, with its count.
    peaks.sent(&news);
    peaks.note(&meters(1), &[reading(0.5, 0.2)]);
    let news = peaks.news();
    assert_eq!(news.len(), 1);
    assert_eq!(news[0].0, left);
    assert_eq!(news[0].1.after, 1);
    assert!(!news[0].1.clips());
}

/// A meter whose probe changed starts with no news, and a meter that left
/// the list is forgotten.
#[test]
fn a_new_probe_or_a_meter_gone_starts_again() {
    let mut peaks = Peaks::default();
    peaks.note(&HashMap::from([(METER, (PROBE, 0))]), &[reading(2.0, 0.5)]);
    let other = ProbeId(2);
    peaks.note(&HashMap::from([(METER, (other, 0))]), &[]);
    assert!(peaks.news().is_empty(), "a new probe starts with no news");
    peaks.note(
        &HashMap::from([(METER, (other, 0))]),
        &[MeterReading {
            probe: other,
            ..reading(2.0, 0.5)
        }],
    );
    peaks.note(&HashMap::new(), &[]);
    assert!(peaks.news().is_empty(), "a meter gone is forgotten");
}
