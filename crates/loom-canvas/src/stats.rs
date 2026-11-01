//! Counters for the work the canvas does: frames drawn, geometry built,
//! and data uploaded to the GPU.
//!
//! They let a script check the drawing rules (ADR-0010) on the running
//! app: an idle patch draws no frames, and turning a knob builds and
//! uploads control data only, never the tiles, ports, and cables.
//!
//! Each count is kept twice: once for the whole process, which the app's
//! frame-rate report reads from its own thread, and once for the current
//! thread, so a test can count its own canvas while other tests run.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// What the canvas has done so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderCounts {
    /// Frames drawn on the GPU.
    pub frames: u64,
    /// Tile, port, and cable geometry built from the scene.
    pub scene_builds: u64,
    /// Panel control geometry built from the scene and its values.
    pub control_builds: u64,
    /// Tile, port, and cable data uploaded to the GPU.
    pub scene_uploads: u64,
    /// Panel control data uploaded to the GPU.
    pub control_uploads: u64,
    /// Overlay data (hover, selection, cable drags) uploaded to the GPU.
    pub overlay_uploads: u64,
    /// Scope traces built from their points.
    pub trace_builds: u64,
    /// Scope trace data uploaded to the GPU.
    pub trace_uploads: u64,
}

/// Message classes that can request a canvas frame (#252).
#[derive(Debug, Clone, Copy)]
pub enum FrameCause {
    Tick,
    Live,
    Meters,
    Control,
    Input,
    Redraw,
    Forced,
    Other,
}

/// Frame totals by cause, in the order printed by the measure report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCauses(pub [u64; 8]);

impl FrameCauses {
    /// Totals for the whole process.
    pub fn process() -> Self {
        Self(std::array::from_fn(|i| CAUSES[i].load(Ordering::Relaxed)))
    }

    /// Counts accumulated since an earlier snapshot.
    pub fn since(self, earlier: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i].saturating_sub(earlier.0[i])))
    }
}

static PENDING_CAUSES: AtomicU64 = AtomicU64::new(0);
static CAUSES: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

/// Marks a message as a cause of the next rendered frame.
pub fn request_frame(cause: FrameCause) {
    PENDING_CAUSES.fetch_or(1 << cause as u8, Ordering::Relaxed);
}

impl RenderCounts {
    /// The counts for the whole process.
    pub fn process() -> Self {
        let read = |i: Counter| GLOBAL[i as usize].load(Ordering::Relaxed);
        Self::from_fn(read)
    }

    /// The counts for the current thread only.
    pub fn this_thread() -> Self {
        LOCAL.with(|local| Self::from_fn(|i| local[i as usize].get()))
    }

    fn from_fn(read: impl Fn(Counter) -> u64) -> Self {
        Self {
            frames: read(Counter::Frames),
            scene_builds: read(Counter::SceneBuilds),
            control_builds: read(Counter::ControlBuilds),
            scene_uploads: read(Counter::SceneUploads),
            control_uploads: read(Counter::ControlUploads),
            overlay_uploads: read(Counter::OverlayUploads),
            trace_builds: read(Counter::TraceBuilds),
            trace_uploads: read(Counter::TraceUploads),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Counter {
    Frames,
    SceneBuilds,
    ControlBuilds,
    SceneUploads,
    ControlUploads,
    OverlayUploads,
    TraceBuilds,
    TraceUploads,
}

const COUNTERS: usize = 8;

static GLOBAL: [AtomicU64; COUNTERS] = [const { AtomicU64::new(0) }; COUNTERS];

thread_local! {
    static LOCAL: [Cell<u64>; COUNTERS] = const { [const { Cell::new(0) }; COUNTERS] };
}

pub(crate) fn count(counter: Counter) {
    GLOBAL[counter as usize].fetch_add(1, Ordering::Relaxed);
    LOCAL.with(|local| {
        let cell = &local[counter as usize];
        cell.set(cell.get() + 1);
    });
}

/// Frames skipped before the longest gap is tracked: they include
/// start-up work.
const WARM_UP: u64 = 10;

/// When the clock for frame gaps started.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// The time of the last frame, in nanoseconds since `EPOCH`.
static LAST_FRAME: AtomicU64 = AtomicU64::new(0);
/// The longest gap between two frames after the warm-up, in nanoseconds.
static LONGEST_GAP: AtomicU64 = AtomicU64::new(0);

/// Counts a frame drawn on the GPU and tracks the longest gap between two.
pub(crate) fn frame() {
    let mut pending = PENDING_CAUSES.swap(0, Ordering::Relaxed);
    if pending == 0 {
        pending = 1 << FrameCause::Redraw as u8;
    }
    for (i, counter) in CAUSES.iter().enumerate() {
        if pending & (1 << i) != 0 {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }
    let epoch = *EPOCH.get_or_init(Instant::now);
    let now = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let last = LAST_FRAME.swap(now, Ordering::Relaxed);
    let seen = GLOBAL[Counter::Frames as usize].load(Ordering::Relaxed);
    if seen >= WARM_UP {
        LONGEST_GAP.fetch_max(now.saturating_sub(last), Ordering::Relaxed);
    }
    count(Counter::Frames);
}

/// The longest gap between two frames drawn in this process, after the
/// first few. With frames drawn back to back, it is the slowest frame.
pub fn longest_frame_gap() -> Duration {
    Duration::from_nanos(LONGEST_GAP.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests;
