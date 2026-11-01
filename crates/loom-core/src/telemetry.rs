//! Data from the audio thread for live views: meters and scopes (FR-VIS-04).
//!
//! The UI asks to watch an output port with [`Controller::add_probe`]. The
//! next [`Controller::commit`] compiles the probe into the schedule. After
//! each internal block, the engine reads the probed buffers and pushes
//! fixed-size messages into two bounded queues: one for meter readings, one
//! for scope samples. The control thread reads them through [`Telemetry`].
//!
//! Telemetry may be lost, edits may not (NFR-RT-07). When a queue is full the
//! engine drops the new message and counts it in
//! [`EngineStats`](crate::EngineStats). It never waits.
//!
//! [`Controller::add_probe`]: crate::Controller::add_probe
//! [`Controller::commit`]: crate::Controller::commit

use rtrb::{Consumer, Producer};
use std::fmt;

/// Number of samples in one [`ScopeChunk`].
pub const SCOPE_CHUNK: usize = 64;

/// Identifies a probe. Returned by [`Controller::add_probe`].
///
/// [`Controller::add_probe`]: crate::Controller::add_probe
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProbeId(pub u32);

impl fmt::Display for ProbeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "probe {}", self.0)
    }
}

/// What a probe sends back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    /// Peak and RMS once per block. Cheap: one small message per block.
    Meter,
    /// Every sample, in chunks of [`SCOPE_CHUNK`]. About 750 messages per
    /// second at 48 kHz, so only probe what is on screen.
    Scope,
    /// Both of the above.
    MeterAndScope,
}

impl ProbeKind {
    /// True if this probe sends meter readings.
    pub fn meter(self) -> bool {
        matches!(self, Self::Meter | Self::MeterAndScope)
    }

    /// True if this probe sends scope samples.
    pub fn scope(self) -> bool {
        matches!(self, Self::Scope | Self::MeterAndScope)
    }
}

/// Peak and RMS of one probed port over one block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterReading {
    /// The probe.
    pub probe: ProbeId,
    /// Engine frame of the block's first sample, counted from engine start.
    pub frame: u64,
    /// Frames in the block.
    pub frames: u32,
    /// Largest absolute sample value.
    pub peak: f32,
    /// Root mean square of the samples.
    pub rms: f32,
    /// Peak and RMS of the right channel, when the port is stereo.
    pub right: Option<(f32, f32)>,
}

/// Consecutive samples from one probed port.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScopeChunk {
    /// The probe.
    pub probe: ProbeId,
    /// Engine frame of `samples[0]`, counted from engine start. A gap between
    /// chunks of the same probe means chunks were dropped.
    pub frame: u64,
    /// Number of valid samples. Blocks are split into chunks of
    /// [`SCOPE_CHUNK`], so the last chunk of a block can be shorter.
    pub len: u32,
    /// The samples. Only the first `len` are valid.
    pub samples: [f32; SCOPE_CHUNK],
}

impl ScopeChunk {
    /// The valid samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples[..self.len as usize]
    }
}

/// Reads telemetry on the control thread. Returned by
/// [`create_engine`](crate::create_engine).
///
/// Drain it regularly, for example once per UI frame. If it is not drained,
/// the queues fill up and new telemetry is dropped. Audio is not affected.
pub struct Telemetry {
    meters: Consumer<MeterReading>,
    scope: Consumer<ScopeChunk>,
}

impl Telemetry {
    pub(crate) fn new(meters: Consumer<MeterReading>, scope: Consumer<ScopeChunk>) -> Self {
        Self { meters, scope }
    }

    /// The oldest waiting meter reading.
    pub fn pop_meter(&mut self) -> Option<MeterReading> {
        self.meters.pop().ok()
    }

    /// The oldest waiting scope chunk.
    pub fn pop_scope(&mut self) -> Option<ScopeChunk> {
        self.scope.pop().ok()
    }

    /// Takes every waiting meter reading, oldest first.
    pub fn meters(&mut self) -> impl Iterator<Item = MeterReading> + '_ {
        std::iter::from_fn(|| self.pop_meter())
    }

    /// Takes every waiting scope chunk, oldest first.
    pub fn scope(&mut self) -> impl Iterator<Item = ScopeChunk> + '_ {
        std::iter::from_fn(|| self.pop_scope())
    }
}

/// The audio-thread end of the telemetry queues.
pub(crate) struct TelemetrySender {
    pub meters: Producer<MeterReading>,
    pub scope: Producer<ScopeChunk>,
}

/// Peak and RMS of a block. Runs on the audio thread.
pub(crate) fn measure(samples: &[f32]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut sum = 0.0f32;
    for &x in samples {
        peak = peak.max(x.abs());
        sum += x * x;
    }
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum / samples.len() as f32).sqrt()
    };
    (peak, rms)
}

#[cfg(test)]
mod tests;
