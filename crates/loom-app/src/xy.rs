//! An XY scope on the telemetry thread: the last pairs of one paired probe, as
//! the points the canvas draws (`docs/design/panel-widgets.md`, XY scope, and
//! `docs/learn/reading-an-xy-scope.md`).

use loom_core::PairChunk;
use std::collections::VecDeque;

/// The most points one trace draws.
pub const MAX_POINTS: usize = 2048;

/// The longest history any `time` asks for, in seconds.
const LONGEST: f32 = 0.1;

/// How finely a point is drawn: a step of 1/2048 of half the frame, far under a
/// pixel. Rounding makes a still signal the same trace every time.
const STEPS: f32 = 2048.0;

/// A value this large is off every range, and is finite, so a rotation of two
/// infinite values cannot make a NaN.
const LARGEST: f32 = 1e30;

/// The last pairs of one probe. Lives on the telemetry thread.
#[derive(Debug)]
pub struct XyTrace {
    /// Each kept pair with its engine frame, oldest first.
    pairs: VecDeque<(u64, [f32; 2])>,
    capacity: usize,
    /// Engine frame of the next expected pair.
    next: Option<u64>,
}

impl XyTrace {
    /// A trace that keeps the longest history at this sample rate.
    pub fn new(sample_rate: f32) -> Self {
        let capacity = (LONGEST * sample_rate).ceil().max(1.0) as usize;
        Self {
            pairs: VecDeque::with_capacity(capacity),
            capacity,
            next: None,
        }
    }

    /// Adds a chunk. If chunks were lost in between, the old pairs are dropped:
    /// a line across the gap would show a shape that is not in the signal. A
    /// pair with a NaN has no place on either axis, so it is dropped too.
    pub fn push(&mut self, chunk: &PairChunk) {
        if self.next.is_some_and(|next| next != chunk.frame) {
            self.pairs.clear();
        }
        self.next = Some(chunk.frame + u64::from(chunk.len));
        for (frame, pair) in (chunk.frame..).zip(chunk.pairs()) {
            if !pair[0].is_nan() && !pair[1].is_nan() {
                self.pairs.push_back((frame, pair));
            }
        }
        let excess = self.pairs.len().saturating_sub(self.capacity);
        self.pairs.drain(..excess);
    }

    /// What the canvas draws of the last `frames` frames: at most
    /// [`MAX_POINTS`] points, keeping every Nth by its frame number, so the
    /// kept points do not shimmer as new ones arrive. `x` runs from 0 to 1
    /// across the frame, and `y` from -1 to 1 up it, both clamped, never
    /// wrapped. With `full` false (middle zoom), only the latest point. A
    /// silent signal is empty, so it draws nothing.
    pub fn trace(&self, frames: usize, range: f32, rotate: bool, full: bool) -> loom_canvas::Trace {
        let frames = frames.max(1) as u64;
        let every = frames.div_ceil(MAX_POINTS as u64);
        // Counted back from the newest kept frame, so a window always holds the
        // same number of points. A count that changed as frames arrive would
        // wake the UI for a still signal.
        let newest = self.next.unwrap_or(0).saturating_sub(1) / every * every;
        let since = newest.saturating_sub((frames / every - 1) * every);
        let recent = || self.pairs.iter().filter(|(frame, _)| *frame >= since);
        if recent().all(|(_, pair)| *pair == [0.0, 0.0]) {
            return loom_canvas::Trace::default();
        }
        let draw = |&(_, pair): &(u64, [f32; 2])| point(pair, range, rotate);
        let points = if full {
            recent().filter(|(frame, _)| frame % every == 0).map(draw).collect()
        } else {
            recent().next_back().map(draw).into_iter().collect()
        };
        loom_canvas::Trace {
            points,
            ..loom_canvas::Trace::default()
        }
    }
}

/// One pair as a point on the frame. Rotation comes before clamping, so a
/// full-scale mono sine reaches past the edge when turned, and stops there.
fn point(pair: [f32; 2], range: f32, rotate: bool) -> [f32; 2] {
    let [x, y] = pair.map(|value| value.clamp(-LARGEST, LARGEST));
    let (across, up) = if rotate {
        // A mono signal, with x equal to y, is then a vertical line.
        (
            (y - x) * std::f32::consts::FRAC_1_SQRT_2,
            (x + y) * std::f32::consts::FRAC_1_SQRT_2,
        )
    } else {
        (x, y)
    };
    let edge = |value: f32| ((value / range).clamp(-1.0, 1.0) * STEPS).round() / STEPS;
    [(edge(across) + 1.0) * 0.5, edge(up)]
}

#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod tests;
