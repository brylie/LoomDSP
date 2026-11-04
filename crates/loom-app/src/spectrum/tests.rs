use super::*;
use loom_core::{ProbeId, SCOPE_CHUNK};

const RATE: f32 = 48_000.0;

/// Feeds samples to an analyser in chunks, from engine frame `from`, and
/// returns the next frame.
fn feed(analyser: &mut SpectrumAnalyser, from: u64, samples: &[f32]) -> u64 {
    let mut frame = from;
    for part in samples.chunks(SCOPE_CHUNK) {
        let mut chunk = ScopeChunk {
            probe: ProbeId(1),
            frame,
            len: part.len() as u32,
            samples: [0.0; SCOPE_CHUNK],
        };
        chunk.samples[..part.len()].copy_from_slice(part);
        analyser.push(&chunk);
        frame += part.len() as u64;
    }
    frame
}

/// `seconds` of a sine, in phase with engine frame `from`.
fn sine(hz: f32, amplitude: f32, from: u64, seconds: f32) -> Vec<f32> {
    let frames = (seconds * RATE) as u64;
    (from..from + frames)
        .map(|frame| amplitude * (std::f64::consts::TAU * f64::from(hz) * frame as f64 / f64::from(RATE)).sin() as f32)
        .collect()
}

/// The dB a trace point stands for.
fn db(y: f32) -> f32 {
    (y + 1.0) * 0.5 * 90.0 - 90.0
}

/// The column a frequency falls in.
fn column_of(hz: f32) -> usize {
    (COLUMNS as f32 * (hz / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln()) as usize
}

/// A 1 kHz sine peaks in the column holding 1 kHz, within one bin
/// (panel-widgets.md, Acceptance criteria).
#[test]
fn a_sine_peaks_in_the_column_that_holds_it() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    feed(&mut analyser, 0, &sine(1000.0, 0.5, 0, 0.5));
    let trace = analyser.trace(false, false);
    let peak = (0..trace.points.len())
        .max_by(|&a, &b| trace.points[a][1].total_cmp(&trace.points[b][1]))
        .unwrap();
    // Within one bin: a column is about 27 Hz wide here, over two bins.
    assert_eq!(peak, column_of(1000.0), "peak at {} Hz", analyser.columns[peak].hz);
}

/// A full-scale sine reads 0 dB, and half of it reads about -6 dB, within the
/// dip a Hann window has between two bins.
#[test]
fn a_full_scale_sine_reads_zero_db() {
    for (amplitude, expected) in [(1.0, 0.0), (0.5, -6.0)] {
        let mut analyser = SpectrumAnalyser::new(RATE);
        // Exactly on a bin, so there is no dip.
        let hz = 85.0 * RATE / SIZE as f32;
        feed(&mut analyser, 0, &sine(hz, amplitude, 0, 0.5));
        let top = analyser.line.iter().copied().fold(FLOOR_DB, f32::max);
        assert!((top - expected).abs() < 0.1, "{amplitude}: {top} dB");
    }
}

/// Nothing draws before the first 4096 samples.
#[test]
fn nothing_draws_before_a_full_analysis() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    feed(&mut analyser, 0, &sine(1000.0, 0.5, 0, 4000.0 / RATE));
    assert_eq!(analyser.trace(false, false), loom_canvas::Trace::default());
    feed(&mut analyser, 4000, &sine(1000.0, 0.5, 4000, 200.0 / RATE));
    assert_eq!(analyser.trace(false, false).points.len(), COLUMNS);
}

/// A silent input draws nothing, so it sends the UI nothing.
#[test]
fn a_silent_input_is_empty() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    feed(&mut analyser, 0, &[0.0; 48_000]);
    assert_eq!(analyser.trace(true, true), loom_canvas::Trace::default());
}

/// After a tone stops, the line falls at 20 dB per second.
#[test]
fn the_line_falls_after_a_peak() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    let frame = feed(&mut analyser, 0, &sine(1000.0, 1.0, 0, 0.5));
    let column = column_of(1000.0);
    let top = analyser.line[column];
    // The tone leaves the window after 4096 samples, about 85 ms.
    feed(&mut analyser, frame, &[0.0; 24_000]);
    let fallen = top - analyser.line[column];
    assert!((6.0..=12.0).contains(&fallen), "fell {fallen} dB in 0.5 s");
}

/// The hold keeps a peak for 2 s, then falls, and only draws when asked.
#[test]
fn hold_keeps_the_peak_for_two_seconds() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    let frame = feed(&mut analyser, 0, &sine(1000.0, 1.0, 0, 0.5));
    let column = column_of(1000.0);
    let top = analyser.hold[column];
    let frame = feed(&mut analyser, frame, &vec![0.0; (1.8 * RATE) as usize]);
    assert_eq!(analyser.hold[column], top, "still held");
    assert!(analyser.line[column] < top - 20.0, "the line has fallen");
    feed(&mut analyser, frame, &vec![0.0; RATE as usize]);
    assert!(analyser.hold[column] < top - 5.0, "the hold falls after 2 s");
    assert!(analyser.trace(false, false).second.is_empty());
    assert_eq!(analyser.trace(false, true).second.len(), COLUMNS);
}

/// One sample that is not finite does not spoil the line.
#[test]
fn a_sample_that_is_not_finite_does_not_spoil_the_line() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    let mut samples = sine(1000.0, 0.5, 0, 0.5);
    samples[6000] = f32::NAN;
    samples[12_000] = f32::INFINITY;
    feed(&mut analyser, 0, &samples);
    let trace = analyser.trace(false, true);
    assert!(trace.points.iter().chain(&trace.second).all(|p| p[1].is_finite()));
    assert!(db(trace.points[column_of(1000.0)][1]) > -12.0);
}

/// Every low column has a level, though most are narrower than a bin, so the
/// line stays continuous (panel-widgets.md, Spectrum, Columns).
#[test]
fn low_columns_are_interpolated() {
    let analyser = SpectrumAnalyser::new(RATE);
    let narrow = analyser
        .columns
        .iter()
        .filter(|c| matches!(c.reads, Reads::Between(..)))
        .count();
    assert!(narrow > 10, "only {narrow} columns hold no bin");
    let mut analyser = SpectrumAnalyser::new(RATE);
    feed(&mut analyser, 0, &sine(40.0, 0.5, 0, 0.5));
    let trace = analyser.trace(false, false);
    let low = &trace.points[..column_of(80.0)];
    for pair in low.windows(2) {
        assert!(db(pair[0][1]) > FLOOR_DB && (db(pair[1][1]) - db(pair[0][1])).abs() < 12.0);
    }
}

/// The line stops at half the sample rate.
#[test]
fn the_line_stops_at_half_the_sample_rate() {
    assert_eq!(columns(48_000.0).len(), COLUMNS);
    let low_rate = columns(22_050.0);
    assert!(low_rate.len() < COLUMNS);
    assert!(low_rate.last().unwrap().hz < 11_025.0 * 1.03);
}

/// A tilt lifts the highs and lowers the lows around 1 kHz, by 4.5 dB per
/// octave, and only in the drawing.
#[test]
fn tilt_changes_only_the_drawing() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    let mut samples = sine(4000.0, 0.1, 0, 0.5);
    for (sample, low) in samples.iter_mut().zip(sine(250.0, 0.1, 0, 0.5)) {
        *sample += low;
    }
    feed(&mut analyser, 0, &samples);
    let line = analyser.line.clone();
    let flat = analyser.trace(false, false);
    let tilted = analyser.trace(true, false);
    assert_eq!(analyser.line, line);
    let lift = |column: usize| db(tilted.points[column][1]) - db(flat.points[column][1]);
    assert!((lift(column_of(4000.0)) - 9.0).abs() < 0.5);
    assert!((lift(column_of(250.0)) + 9.0).abs() < 0.5);
}

/// A gap in the chunks drops the waiting samples, so an analysis never spans
/// one.
#[test]
fn a_gap_drops_the_waiting_samples() {
    let mut analyser = SpectrumAnalyser::new(RATE);
    feed(&mut analyser, 0, &sine(1000.0, 0.5, 0, 2000.0 / RATE));
    feed(&mut analyser, 10_000, &sine(1000.0, 0.5, 10_000, 3000.0 / RATE));
    assert_eq!(analyser.trace(false, false), loom_canvas::Trace::default());
}
