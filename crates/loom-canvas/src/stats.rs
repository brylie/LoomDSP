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
}

const COUNTERS: usize = 6;

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
mod tests {
    use super::*;

    #[test]
    fn counts_for_the_thread_and_the_process() {
        let process = RenderCounts::process();
        let thread = RenderCounts::this_thread();
        count(Counter::ControlBuilds);
        assert_eq!(RenderCounts::this_thread().control_builds, thread.control_builds + 1);
        assert_eq!(RenderCounts::this_thread().scene_builds, thread.scene_builds);
        assert!(RenderCounts::process().control_builds > process.control_builds);
    }
}
