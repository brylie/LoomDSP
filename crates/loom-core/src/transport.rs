//! The patch transport: tempo, time signature, play state, and position
//! (ADR-0015). Spec: `docs/design/clock.md`.
//!
//! Position is computed from an anchor, never by adding a step per block:
//! `anchor_position + (sample - anchor_sample) × beats_per_sample`. Adding a
//! step per block would round differently at different block sizes. With an
//! anchor, a given sample gets the same position for any block size, bit for
//! bit, and rounding does not build up over an hour-long set.

use std::fmt;

/// Slowest tempo, in beats per minute.
pub const MIN_TEMPO: f64 = 20.0;
/// Fastest tempo, in beats per minute.
pub const MAX_TEMPO: f64 = 300.0;
/// Tempo of a new engine, in beats per minute.
pub const DEFAULT_TEMPO: f64 = 120.0;

/// Module ID of the Transport module, the patch's one control surface for
/// the transport (`docs/design/clock.md`, Controls live on the Transport
/// module). The controller keeps a patch to one of it, and the engine asks
/// it for transport changes ([`TransportRequest`]).
pub const TRANSPORT_MODULE: &str = "loom.clock.transport";

/// Frames in one step of the grid on which the Transport's `run` and
/// `reset` inputs are read (rule 11). With a cable on either input, the
/// engine splits every block at multiples of this many frames, counted from
/// engine start, and applies what the module found at the next multiple.
pub const TRANSPORT_GRID: usize = 32;

/// Changes the Transport module asks the engine to make before the next
/// block runs. See [`Module::transport_request`](crate::Module::transport_request).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TransportRequest {
    /// A new tempo, in beats per minute. Clamped by the engine.
    pub tempo: Option<f64>,
    /// A new time signature. An invalid one is ignored.
    pub time_signature: Option<TimeSignature>,
    /// Start (true) or stop (false).
    pub running: Option<bool>,
    /// Return to bar 1, as a jump.
    pub reset: bool,
}

impl TransportRequest {
    /// True when it asks for nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Clamps a tempo to [`MIN_TEMPO`]..=[`MAX_TEMPO`], as a parameter is
/// clamped. NaN becomes [`DEFAULT_TEMPO`].
pub fn clamp_tempo(bpm: f64) -> f64 {
    if bpm.is_nan() {
        DEFAULT_TEMPO
    } else {
        bpm.clamp(MIN_TEMPO, MAX_TEMPO)
    }
}

/// A time signature, such as 4/4 or 7/8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    /// Beats in a bar, 1 to 32.
    pub numerator: u32,
    /// Note value of a beat: 1, 2, 4, 8, or 16.
    pub denominator: u32,
}

impl TimeSignature {
    /// Common time.
    pub const FOUR_FOUR: Self = Self::new(4, 4);

    /// Creates a time signature. Check it with [`TimeSignature::is_valid`].
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        Self { numerator, denominator }
    }

    /// True when the numerator is 1 to 32 and the denominator is 1, 2, 4,
    /// 8, or 16.
    pub fn is_valid(self) -> bool {
        (1..=32).contains(&self.numerator) && matches!(self.denominator, 1 | 2 | 4 | 8 | 16)
    }

    /// Length of a bar in quarter-note beats: `numerator × 4 / denominator`.
    pub fn beats_per_bar(self) -> f64 {
        f64::from(self.numerator) * 4.0 / f64::from(self.denominator)
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self::FOUR_FOUR
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

/// The transport settings that the control thread sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportSettings {
    /// Quarter-note beats per minute.
    pub tempo: f64,
    /// Sets the length of a bar.
    pub time_signature: TimeSignature,
    /// True while the transport runs.
    pub running: bool,
}

impl Default for TransportSettings {
    /// Running, at 120 BPM, in 4/4: how a new engine starts.
    fn default() -> Self {
        Self {
            tempo: DEFAULT_TEMPO,
            time_signature: TimeSignature::FOUR_FOUR,
            running: true,
        }
    }
}

/// The transport as one call to [`Module::process`](crate::Module::process)
/// sees it. Read it with [`ProcessContext::transport`](crate::ProcessContext::transport).
///
/// Use [`TransportInfo::position_at`] (or the phase helpers) for the value at
/// a frame inside the call. Adding `i × beats_per_sample` to the position
/// yourself rounds differently at different block sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportInfo {
    playing: bool,
    tempo: f64,
    beats_per_bar: f64,
    beats_per_sample: f64,
    anchor_position: f64,
    /// Frames from the anchor to the first frame of this call.
    offset: u64,
    jumped: bool,
}

impl TransportInfo {
    /// The transport at `position` beats, for testing modules alone. It has
    /// not jumped; see [`TransportInfo::with_jump`].
    ///
    /// Tempo is clamped, as the engine does. A time signature that
    /// [`TimeSignature::is_valid`] refuses has no sensible nearest value, and
    /// a numerator of 0 would make every bar phase NaN. So this is a
    /// programming error in the test, and it panics with a clear message.
    ///
    /// # Panics
    ///
    /// Panics when `settings.time_signature` is not valid.
    pub fn new(sample_rate: f32, settings: TransportSettings, position: f64) -> Self {
        assert!(
            settings.time_signature.is_valid(),
            "invalid time signature {}: the numerator must be 1 to 32 and the denominator 1, 2, 4, 8, or 16",
            settings.time_signature
        );
        let tempo = clamp_tempo(settings.tempo);
        Self {
            playing: settings.running,
            tempo,
            beats_per_bar: settings.time_signature.beats_per_bar(),
            beats_per_sample: beats_per_sample(settings.running, tempo, sample_rate),
            anchor_position: position,
            offset: 0,
            jumped: false,
        }
    }

    /// Marks the first frame of this call as a jump, for testing modules.
    pub fn with_jump(mut self, jumped: bool) -> Self {
        self.jumped = jumped;
        self
    }

    /// True while the transport runs.
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Quarter-note beats per minute.
    pub fn tempo(&self) -> f64 {
        self.tempo
    }

    /// Position, in quarter-note beats since bar 1, at the first frame of
    /// this call.
    pub fn position(&self) -> f64 {
        self.position_at(0)
    }

    /// Position at frame `frame` of this call, computed exactly from the
    /// transport's anchor. The same frame gets the same value for any block
    /// size.
    pub fn position_at(&self, frame: usize) -> f64 {
        self.anchor_position + (self.offset + frame as u64) as f64 * self.beats_per_sample
    }

    /// How far the position moves per frame. 0 while stopped, so a module
    /// that computes from it holds still with no special case.
    pub fn beats_per_sample(&self) -> f64 {
        self.beats_per_sample
    }

    /// Length of a bar in quarter-note beats.
    pub fn beats_per_bar(&self) -> f64 {
        self.beats_per_bar
    }

    /// True when the position jumped (a reset) at the first frame of this
    /// call.
    pub fn jumped(&self) -> bool {
        self.jumped
    }

    /// Beat phase at frame `frame` of this call, from 0 up to 1.
    pub fn beat_phase_at(&self, frame: usize) -> f32 {
        phase(self.position_at(frame))
    }

    /// Bar phase at frame `frame` of this call, from 0 up to 1.
    pub fn bar_phase_at(&self, frame: usize) -> f32 {
        self.phase_at(frame, self.beats_per_bar)
    }

    /// Phase of a cycle `beats` beats long at frame `frame` of this call,
    /// from 0 up to 1. Cycles are counted from position 0, so every rate
    /// stays aligned with bar 1. This is the rate phase of `docs/design/clock.md`.
    ///
    /// It divides the position by `beats` in `f64` and converts last, as the
    /// bar phase does, so a cycle of exactly one bar gives the bar phase bit
    /// for bit.
    pub fn phase_at(&self, frame: usize, beats: f64) -> f32 {
        phase(self.position_at(frame) / beats)
    }

    /// The transport for a call that starts `frames` frames later. Only a
    /// call that starts on the jump frame sees the jump.
    ///
    /// The engine uses this to split a block into calls. A test harness that
    /// splits a render into blocks must use it too: positions then come from
    /// the same anchor, bit for bit, at any block size.
    pub fn later(self, frames: usize) -> Self {
        Self {
            offset: self.offset + frames as u64,
            jumped: self.jumped && frames == 0,
            ..self
        }
    }
}

impl Default for TransportInfo {
    /// Stopped at position 0, at 120 BPM, in 4/4.
    fn default() -> Self {
        Self::new(
            48_000.0,
            TransportSettings {
                running: false,
                ..TransportSettings::default()
            },
            0.0,
        )
    }
}

fn beats_per_sample(running: bool, tempo: f64, sample_rate: f32) -> f64 {
    if running {
        tempo / 60.0 / f64::from(sample_rate)
    } else {
        0.0
    }
}

/// The fractional part of `x` as an `f32` phase. A value that rounds to 1.0
/// becomes 0.0, so a phase is always in [0, 1). The wrap then comes less
/// than one `f32` step early, far less than one sample.
pub(crate) fn phase(x: f64) -> f32 {
    let p = (x - x.floor()) as f32;
    if p >= 1.0 { 0.0 } else { p }
}

/// The engine's transport. Lives on the audio thread and changes only at
/// buffer starts, through commands from the controller.
pub(crate) struct Transport {
    settings: TransportSettings,
    sample_rate: f32,
    anchor_sample: u64,
    anchor_position: f64,
    /// A reset happened. The next block starts with a jump.
    jump_pending: bool,
}

impl Transport {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            settings: TransportSettings::default(),
            sample_rate,
            anchor_sample: 0,
            anchor_position: 0.0,
            jump_pending: false,
        }
    }

    /// The transport for a block whose first frame is `sample` (frames since
    /// the engine started).
    pub fn info(&self, sample: u64) -> TransportInfo {
        let tempo = self.settings.tempo;
        TransportInfo {
            playing: self.settings.running,
            tempo,
            beats_per_bar: self.settings.time_signature.beats_per_bar(),
            beats_per_sample: beats_per_sample(self.settings.running, tempo, self.sample_rate),
            anchor_position: self.anchor_position,
            offset: sample - self.anchor_sample,
            jumped: self.jump_pending,
        }
    }

    /// Moves the anchor to `sample`, keeping the position there.
    fn re_anchor(&mut self, sample: u64) {
        self.anchor_position = self.info(sample).position();
        self.anchor_sample = sample;
    }

    /// Changes the speed from `sample` on. The position does not jump.
    pub fn set_tempo(&mut self, sample: u64, tempo: f64) {
        self.re_anchor(sample);
        self.settings.tempo = tempo;
    }

    /// Changes the bar length. The position does not move, but the bar
    /// grid does, so a change is a jump (rule 10). The same time signature
    /// again changes nothing.
    pub fn set_time_signature(&mut self, time_signature: TimeSignature) {
        debug_assert!(
            time_signature.is_valid(),
            "the Controller refuses invalid time signatures"
        );
        if time_signature != self.settings.time_signature {
            self.settings.time_signature = time_signature;
            self.jump_pending = true;
        }
    }

    /// Applies what the Transport module asked for, at `sample`: time
    /// signature, tempo, running, then reset.
    pub fn apply(&mut self, sample: u64, request: TransportRequest) {
        if let Some(time_signature) = request.time_signature.filter(|t| t.is_valid()) {
            self.set_time_signature(time_signature);
        }
        if let Some(tempo) = request.tempo {
            let tempo = clamp_tempo(tempo);
            if tempo != self.settings.tempo {
                self.set_tempo(sample, tempo);
            }
        }
        if let Some(running) = request.running {
            self.set_running(sample, running);
        }
        if request.reset {
            self.reset(sample);
        }
    }

    /// Starts or stops at `sample`. Stopping holds the position.
    pub fn set_running(&mut self, sample: u64, running: bool) {
        if running != self.settings.running {
            self.re_anchor(sample);
            self.settings.running = running;
        }
    }

    /// Returns to bar 1 at `sample`, running or stopped, and marks a jump.
    pub fn reset(&mut self, sample: u64) {
        self.anchor_position = 0.0;
        self.anchor_sample = sample;
        self.jump_pending = true;
    }

    /// Called after each block: a jump lasts one frame.
    pub fn end_block(&mut self) {
        self.jump_pending = false;
    }
}

#[cfg(test)]
mod tests;
