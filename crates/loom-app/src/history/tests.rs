use super::*;
use loom_core::ProbeId;

/// Block length in frames, as the engine's default.
const BLOCK: u32 = 128;
const RATE: f32 = 48_000.0;

/// A mono reading at engine frame `frame`.
fn reading(frame: u64, rms: f32) -> MeterReading {
    MeterReading {
        probe: ProbeId(1),
        frame,
        frames: BLOCK,
        peak: rms * std::f32::consts::SQRT_2,
        rms,
        right: None,
    }
}

/// Feeds `seconds` of steady level from `from`, and returns the next frame.
fn run(history: &mut LevelHistory, from: u64, seconds: f32, rms: f32) -> u64 {
    let blocks = (seconds * RATE / BLOCK as f32) as u64;
    for i in 0..blocks {
        history.push(&reading(from + i * u64::from(BLOCK), rms));
    }
    from + blocks * u64::from(BLOCK)
}

/// The history's newest column reads the level, in a trace's vertical units:
/// -1 at -60 dB and 1 at 0 dB.
#[test]
fn a_steady_level_fills_the_columns() {
    let mut history = LevelHistory::new(1.0, RATE);
    run(&mut history, 0, 2.0, 0.1);
    let trace = history.trace();
    assert_eq!(trace.points.len(), COLUMNS);
    assert_eq!(trace.second.len(), COLUMNS);
    // 0.1 is -20 dB, two thirds of the way up the scale.
    let expected = 2.0 * (40.0 / 60.0) - 1.0;
    assert!((trace.points[COLUMNS - 1][1] - expected).abs() < 0.01);
    assert!(
        trace.second[COLUMNS - 1][1] > trace.points[COLUMNS - 1][1],
        "peak above RMS"
    );
    assert!(trace.points[COLUMNS - 1][0] < 1.0, "newest at the right");
    assert!(trace.points[0][0] < trace.points[1][0], "oldest at the left");
}

/// A tone that stops scrolls away with none of the meter's fall: RMS reaches
/// the floor one window later (panel-widgets.md, Acceptance criteria).
#[test]
fn a_tone_that_stops_reaches_the_floor_over_the_window() {
    let mut history = LevelHistory::new(1.0, RATE);
    let frame = run(&mut history, 0, 1.5, 0.5);
    let frame = run(&mut history, frame, 0.5, 0.0);
    let trace = history.trace();
    let at = |column: usize| trace.points[column][1];
    assert_eq!(at(COLUMNS - 1), -1.0, "the newest column is at the floor at once");
    assert!(at(10) > 0.0, "the old part of the window still shows the tone");
    run(&mut history, frame, 0.6, 0.0);
    assert_eq!(
        history.trace(),
        loom_canvas::Trace::default(),
        "one window later, nothing"
    );
}

/// A silent input draws nothing, so it sends the UI nothing.
#[test]
fn a_silent_history_is_empty() {
    let mut history = LevelHistory::new(5.0, RATE);
    run(&mut history, 0, 6.0, 0.0);
    assert_eq!(history.trace(), loom_canvas::Trace::default());
}

/// A reading that is not finite counts for nothing, and the columns around it
/// keep their level.
#[test]
fn a_reading_that_is_not_finite_is_skipped() {
    let mut history = LevelHistory::new(1.0, RATE);
    let frame = run(&mut history, 0, 0.5, 0.1);
    history.push(&MeterReading {
        peak: f32::INFINITY,
        rms: f32::NAN,
        ..reading(frame, 0.0)
    });
    run(&mut history, frame + u64::from(BLOCK), 0.5, 0.1);
    let trace = history.trace();
    assert!(trace.points.iter().chain(&trace.second).all(|p| p[1].is_finite()));
    assert!(trace.second.iter().all(|p| p[1] < 1.0), "the bad peak left no mark");
}

/// A column no reading reached shows the floor, because readings are placed
/// by their frame stamps.
#[test]
fn a_gap_in_the_readings_shows_the_floor() {
    let mut history = LevelHistory::new(1.0, RATE);
    let frame = run(&mut history, 0, 0.5, 0.5);
    run(&mut history, frame + (0.2 * RATE) as u64, 0.3, 0.5);
    let trace = history.trace();
    assert!(trace.points.iter().any(|p| p[1] == -1.0), "the gap");
    assert!(trace.points.last().unwrap()[1] > 0.0, "the newest columns");
}

/// A stereo output uses the higher side in each column.
#[test]
fn a_stereo_output_uses_the_higher_side() {
    let mut history = LevelHistory::new(1.0, RATE);
    let mut quiet_left = LevelHistory::new(1.0, RATE);
    for i in 0..1000u64 {
        let frame = i * u64::from(BLOCK);
        history.push(&reading(frame, 0.5));
        quiet_left.push(&MeterReading {
            right: Some((0.5 * std::f32::consts::SQRT_2, 0.5)),
            ..reading(frame, 0.01)
        });
    }
    assert_eq!(history.trace().points, quiet_left.trace().points);
}

/// A history keeps its window, and only the columns of it.
#[test]
fn a_long_run_stays_at_the_column_count() {
    let mut history = LevelHistory::new(1.0, RATE);
    run(&mut history, 0, 30.0, 0.1);
    assert_eq!(history.window(), 1.0);
    assert_eq!(history.trace().points.len(), COLUMNS);
}
