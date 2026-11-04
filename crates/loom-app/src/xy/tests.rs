use super::*;
use loom_core::{PAIR_CHUNK, ProbeId};

const RATE: f32 = 48_000.0;

/// Feeds pairs in chunks from engine frame `from`, and returns the next frame.
fn feed(trace: &mut XyTrace, from: u64, pairs: &[[f32; 2]]) -> u64 {
    let mut frame = from;
    for part in pairs.chunks(PAIR_CHUNK) {
        let mut chunk = PairChunk {
            probe: ProbeId(1),
            frame,
            len: part.len() as u32,
            values: [0.0; PAIR_CHUNK * 2],
        };
        for (out, pair) in chunk.values.as_chunks_mut::<2>().0.iter_mut().zip(part) {
            out.copy_from_slice(pair);
        }
        trace.push(&chunk);
        frame += part.len() as u64;
    }
    frame
}

/// `frames` pairs of two sines at 480 Hz, `phase` of a cycle apart.
fn sines(frames: usize, amplitude: f32, phase: f32) -> Vec<[f32; 2]> {
    (0..frames)
        .map(|i| {
            let at = i as f32 * 480.0 / RATE;
            [
                amplitude * (std::f32::consts::TAU * at).sin(),
                amplitude * (std::f32::consts::TAU * (at + phase)).sin(),
            ]
        })
        .collect()
}

/// Two sines a quarter cycle apart draw a circle: every point is the same
/// distance from the middle (panel-widgets.md, XY scope).
#[test]
fn a_quarter_cycle_apart_draws_a_circle() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &sines(960, 0.5, 0.25));
    let points = trace.trace(100, 1.0, false, true).points;
    assert_eq!(points.len(), 100, "one cycle");
    for [x, y] in points {
        let across = x * 2.0 - 1.0;
        let radius = (across * across + y * y).sqrt();
        assert!((radius - 0.5).abs() < 0.002, "radius {radius}");
    }
}

/// Equal signals draw a diagonal, and with `rotate`, a vertical line.
#[test]
fn equal_signals_draw_a_diagonal_and_rotate_to_a_vertical_line() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &sines(960, 0.5, 0.0));
    for [x, y] in trace.trace(100, 1.0, false, true).points {
        assert!((x * 2.0 - 1.0 - y).abs() < 0.001, "diagonal: {x} {y}");
    }
    let turned = trace.trace(100, 1.0, true, true).points;
    assert!(turned.iter().all(|p| (p[0] - 0.5).abs() < 0.001), "vertical");
    assert!(turned.iter().any(|p| p[1] > 0.6), "0.5 reaches 0.71 up");
}

/// A signal only on `x` leans up and to the left when rotated.
#[test]
fn a_signal_on_x_alone_leans_up_and_left() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &[[0.5, 0.0]; 64]);
    let [x, y] = trace.trace(10, 1.0, true, true).points[0];
    assert!(x < 0.5, "left of the middle: {x}");
    assert!(y > 0.0, "above the middle: {y}");
}

/// Points outside the range clamp to the edge and never wrap. Rotation comes
/// first, so a full-scale mono sine clamps at the top and bottom.
#[test]
fn points_clamp_to_the_edge() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &[[5.0, -5.0]; 64]);
    assert_eq!(trace.trace(10, 1.0, false, true).points[0], [1.0, -1.0]);
    assert_eq!(trace.trace(10, 10.0, false, true).points[0], [0.75, -0.5]);
    let mut mono = XyTrace::new(RATE);
    feed(&mut mono, 0, &[[1.0, 1.0]; 64]);
    let [x, y] = mono.trace(10, 1.0, true, true).points[0];
    assert_eq!((x, y), (0.5, 1.0), "1.41 reaches the top edge");
}

/// A pair with a NaN is dropped, and infinity clamps to the edge, also when a
/// rotation would subtract two of them.
#[test]
fn values_that_are_not_finite() {
    let mut trace = XyTrace::new(RATE);
    feed(
        &mut trace,
        0,
        &[[0.5, 0.5], [f32::NAN, 0.5], [0.5, f32::NAN], [f32::INFINITY, 0.0]],
    );
    let points = trace.trace(100, 1.0, false, true).points;
    assert_eq!(points.len(), 2, "the two pairs with a NaN are gone");
    assert_eq!(points[1], [1.0, 0.0]);
    let mut both = XyTrace::new(RATE);
    feed(&mut both, 0, &[[f32::INFINITY, f32::INFINITY]]);
    let points = both.trace(100, 1.0, true, true).points;
    assert!(points[0].iter().all(|v| v.is_finite()));
    assert_eq!(points[0], [0.5, 1.0]);
}

/// When `time` holds more frames than the limit, every Nth pair is kept: at
/// 48 kHz, 0.1 s is 4800 frames, so N is 3 (panel-widgets.md, XY scope).
#[test]
fn a_long_time_keeps_every_nth_pair() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &sines(9600, 0.5, 0.25));
    let points = trace.trace(4800, 1.0, false, true).points;
    assert!(points.len() <= MAX_POINTS, "{}", points.len());
    assert_eq!(points.len(), 1600);
}

/// A gap in the chunks clears the history, so no line joins two blocks that
/// were not next to each other.
#[test]
fn a_gap_clears_the_history() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &[[0.5, 0.5]; 64]);
    feed(&mut trace, 1000, &[[-0.5, -0.5]; 32]);
    let points = trace.trace(480, 1.0, false, true).points;
    assert_eq!(points.len(), 32);
    assert!(points.iter().all(|p| p[1] < 0.0));
}

/// At the middle zoom, one point at the latest position.
#[test]
fn the_middle_zoom_is_one_point() {
    let mut trace = XyTrace::new(RATE);
    feed(&mut trace, 0, &[[0.5, 0.5], [-0.5, 0.25]]);
    assert_eq!(trace.trace(100, 1.0, false, false).points, [[0.25, 0.25]]);
}

/// A silent signal draws nothing, and a still one the same trace every time.
#[test]
fn silent_and_still_signals() {
    let mut silent = XyTrace::new(RATE);
    feed(&mut silent, 0, &[[0.0, 0.0]; 512]);
    assert_eq!(silent.trace(100, 1.0, false, true), loom_canvas::Trace::default());
    let mut still = XyTrace::new(RATE);
    let frame = feed(&mut still, 0, &[[0.25, 0.5]; 256]);
    let before = still.trace(100, 1.0, false, true);
    feed(&mut still, frame, &[[0.25, 0.5]; 256]);
    assert_eq!(still.trace(100, 1.0, false, true), before);
}

/// A still signal gives the same trace at every step, also when `time` holds a
/// number of frames that does not divide by the kept step N. Otherwise the point
/// count changes as frames arrive, and each change wakes the UI (rule 7).
#[test]
fn a_still_signal_keeps_one_trace_when_every_nth_pair_is_kept() {
    for (rate, frames) in [(48_000.0, 4181), (96_000.0, 8362), (44_100.0, 4300), (48_000.0, 4800)] {
        let mut trace = XyTrace::new(rate);
        let mut frame = feed(&mut trace, 0, &vec![[0.25, 0.5]; 2 * rate as usize / 10]);
        let first = trace.trace(frames, 1.0, false, true);
        assert!(!first.points.is_empty());
        for _ in 0..19 {
            frame = feed(&mut trace, frame, &[[0.25, 0.5]; 128]);
            assert_eq!(
                trace.trace(frames, 1.0, false, true),
                first,
                "{rate} Hz, {frames} frames"
            );
        }
    }
}
