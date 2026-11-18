//! Bounded distance checks for XY traces that can turn back on either axis.

use super::{DrawnSize, SCOPE_TOLERANCE, distance_to_segment};

/// A grid narrows the search to segments whose bounds reach nearby cells.
const CELLS: usize = 16;
/// Maximum cell entries per segment on average, across the whole grid.
const GRID_BUDGET: usize = crate::xy::MAX_POINTS * CELLS;
/// Maximum candidate segments tried for one point.
const POINT_BUDGET: usize = 64;

/// True if either trace has a point more than half a pixel from the other's
/// segments. An over-budget search sends the new trace.
pub(super) fn moved(a: &[[f32; 2]], b: &[[f32; 2]], drawn: Option<DrawnSize>) -> bool {
    if a == b {
        return false;
    }
    let Some(size) = drawn else {
        return true;
    };
    if a.is_empty() || b.is_empty() {
        return true;
    }
    let frame = [size.width as f32, size.height as f32];
    let pixels = |p: [f32; 2]| [p[0] * frame[0], (p[1] + 1.0) * 0.5 * frame[1]];
    let a: Vec<_> = a.iter().copied().map(pixels).collect();
    let b: Vec<_> = b.iter().copied().map(pixels).collect();
    let off_line = |points: &[[f32; 2]], line| {
        Grid::new(line, frame).is_none_or(|grid| points.iter().any(|&point| !grid.near(point)))
    };
    off_line(&a, &b) || off_line(&b, &a)
}

/// Segments indexed by the cells touched by their bounding boxes.
struct Grid<'a> {
    line: &'a [[f32; 2]],
    frame: [f32; 2],
    cells: [Vec<usize>; CELLS * CELLS],
}

impl<'a> Grid<'a> {
    /// Indexes the whole line, returning none if the grid would be too dense.
    fn new(line: &'a [[f32; 2]], frame: [f32; 2]) -> Option<Self> {
        let mut grid = Self {
            line,
            frame,
            cells: std::array::from_fn(|_| Vec::new()),
        };
        let mut left = GRID_BUDGET;
        for i in 0..line.len().saturating_sub(1).max(1) {
            let (a, b) = grid.segment(i);
            let low = [a[0].min(b[0]), a[1].min(b[1])];
            let high = [a[0].max(b[0]), a[1].max(b[1])];
            for cell in Self::indices(frame, low, high) {
                left = left.checked_sub(1)?;
                grid.cells[cell].push(i);
            }
        }
        Some(grid)
    }

    /// The cells touched by a rectangle in physical pixels.
    fn indices(frame: [f32; 2], low: [f32; 2], high: [f32; 2]) -> impl Iterator<Item = usize> {
        let cell = |value: f32, extent| ((value / extent * CELLS as f32) as usize).min(CELLS - 1);
        let x = cell(low[0], frame[0])..=cell(high[0], frame[0]);
        let y = cell(low[1], frame[1])..=cell(high[1], frame[1]);
        y.flat_map(move |y| x.clone().map(move |x| y * CELLS + x))
    }

    /// Reads a segment, with a single point treated as a dot.
    fn segment(&self, i: usize) -> ([f32; 2], [f32; 2]) {
        (self.line[i], self.line[(i + 1).min(self.line.len() - 1)])
    }

    /// Finds a segment within the tolerance, stopping at the work limit.
    fn near(&self, point: [f32; 2]) -> bool {
        let low = point.map(|v| v - SCOPE_TOLERANCE);
        let high = point.map(|v| v + SCOPE_TOLERANCE);
        Self::indices(self.frame, low, high)
            .flat_map(|cell| self.cells[cell].iter().copied())
            .take(POINT_BUDGET)
            .any(|i| {
                let (a, b) = self.segment(i);
                distance_to_segment(point, a, b) <= SCOPE_TOLERANCE
            })
    }
}
