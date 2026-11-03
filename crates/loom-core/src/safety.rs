//! The safety stage on the engine's final output (ADR-0019).
//!
//! Spec: `docs/design/output-safety.md`. Why each step exists:
//! `docs/learn/output-safety.md`.
//!
//! Every step advances one sample at a time, so the output is the same for
//! any block size. All memory is allocated when the stage is built, so
//! nothing here allocates on the audio thread, not even a mode change.

use loom_dsp::filter::dc_blocker::DcBlocker;
use loom_dsp::limiter::{self, Limiter};

/// What the safety stage does to the final output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputSafety {
    /// DC blocker and a limiter with no lookahead. Adds no latency, and can
    /// distort briefly while it catches a peak. The default.
    #[default]
    LowLatency,
    /// DC blocker and a limiter with a short lookahead. Transparent, and
    /// adds [`LOOKAHEAD_SECONDS`] of latency.
    Clean,
    /// No DC blocker and no limiter: the raw output, clipped to -1 to 1.
    Off,
}

impl OutputSafety {
    /// Every mode, in the order a menu lists them.
    pub const ALL: [Self; 3] = [Self::LowLatency, Self::Clean, Self::Off];

    /// The mode's name on the command line: `low-latency`, `clean`, or `off`.
    pub fn name(self) -> &'static str {
        match self {
            Self::LowLatency => "low-latency",
            Self::Clean => "clean",
            Self::Off => "off",
        }
    }
}

impl std::fmt::Display for OutputSafety {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for OutputSafety {
    type Err = String;

    /// Reads a mode by its [`name`](Self::name).
    fn from_str(name: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == name)
            .ok_or_else(|| format!("unknown output safety mode \"{name}\": use low-latency, clean, or off"))
    }
}

/// The limiter's ceiling: -1 dBFS, sample peak (rule 4).
pub const CEILING: f32 = 0.891_250_9;
/// The DC blocker's cut-off in Hz (rule 5).
pub const DC_CUTOFF_HZ: f32 = 5.0;
/// How long the limiter takes to recover, as a time constant (spec, Modes).
pub const RELEASE_SECONDS: f32 = 0.1;
/// How far ahead the limiter looks in [`OutputSafety::Clean`] mode. It is
/// also the latency that mode adds.
pub const LOOKAHEAD_SECONDS: f32 = 0.002;
/// How long the fade-in, and each half of a mode change, take (rules 8, 9).
pub const FADE_SECONDS: f32 = 0.01;
/// Inputs are clamped to this before the filter, so a huge finite value can
/// never overflow its state to infinity (rule 7).
const INPUT_LIMIT: f32 = 16.0;

/// Converts seconds to whole samples at `sample_rate`.
pub fn samples(seconds: f32, sample_rate: f32) -> usize {
    (seconds * sample_rate).round() as usize
}

/// The safety stage, for one stereo output.
pub(crate) struct SafetyStage {
    mode: OutputSafety,
    /// A mode waiting for the fade to reach 0 (rule 9).
    pending: Option<OutputSafety>,
    /// True after [`SafetyStage::fade_out`]: the fade goes to 0 and stays
    /// there (rule 14).
    stopping: bool,
    /// True once the stage is stopping and its last sample was 0.
    silent: bool,
    dc: [DcBlocker; 2],
    limiter: Limiter,
    fade: Fade,
}

impl SafetyStage {
    pub(crate) fn new(mode: OutputSafety, sample_rate: f32) -> Self {
        let mut stage = Self {
            mode,
            pending: None,
            stopping: false,
            silent: false,
            dc: [dc_blocker(sample_rate), dc_blocker(sample_rate)],
            limiter: limiter(sample_rate),
            fade: Fade::new(sample_rate),
        };
        stage.limiter.set_mode(limiter_mode(mode));
        stage.fade_in();
        stage
    }

    /// Starts the fade-in. In Clean mode the delay line is still silent, so
    /// the fade waits until the first real sample comes out of it. A fade
    /// over silence would meet that sample part way up, as a step.
    fn fade_in(&mut self) {
        self.fade.rising = true;
        self.fade.hold = if self.mode == OutputSafety::Clean {
            self.limiter.lookahead()
        } else {
            0
        };
    }

    /// Restarts the fade after a paused output stream resumes (rule 15).
    /// A requested stop remains silent and must never be cancelled here.
    pub(crate) fn restart_fade(&mut self) {
        if !self.stopping {
            self.fade.position = 0;
            self.fade_in();
        }
    }

    /// Changes the mode. The output dips to silence, the mode changes, and
    /// it fades back in.
    pub(crate) fn set_mode(&mut self, mode: OutputSafety) {
        if self.stopping || mode == self.pending.unwrap_or(self.mode) {
            return;
        }
        self.pending = Some(mode);
        self.fade.rising = false;
    }

    /// Fades the output to 0 from the gain it has now, and keeps it there
    /// (rule 14). A mode change on its way is dropped: nothing fades back in.
    pub(crate) fn fade_out(&mut self) {
        self.stopping = true;
        self.pending = None;
        self.fade.rising = false;
        // A Clean fade-in still waiting for its first sample is at 0 already.
        self.fade.hold = 0;
    }

    /// Starts a panic fade (panic.md, step 1): the gain falls to 0 from
    /// where it is now, one step per frame, whatever the mode. Returns how
    /// many frames are left before the gain is 0, so the engine can reset
    /// the modules at that frame. `None` while the stage is stopping: the
    /// output is going silent anyway and must not come back.
    pub(crate) fn panic_fade(&mut self) -> Option<usize> {
        if self.stopping {
            return None;
        }
        self.fade.rising = false;
        // A Clean fade-in still waiting for its first sample is at 0 already.
        self.fade.hold = 0;
        Some(self.fade.position)
    }

    /// Clears the stage at the silent frame of a panic and fades back in
    /// (panic.md, steps 3 and 4). A mode change that was waiting takes
    /// effect now, as it would at any silence.
    pub(crate) fn panic_reset(&mut self) {
        if let Some(mode) = self.pending.take() {
            self.mode = mode;
        }
        self.dc.iter_mut().for_each(DcBlocker::reset);
        self.limiter.set_mode(limiter_mode(self.mode));
        self.fade.position = 0;
        self.fade_in();
    }

    /// True when the fade-in is complete and the gain is 1.
    pub(crate) fn is_open(&self) -> bool {
        self.fade.rising && self.fade.hold == 0 && self.fade.position == self.fade.length
    }

    /// True once [`SafetyStage::fade_out`] was called and the last sample
    /// out was silent.
    pub(crate) fn is_silent(&self) -> bool {
        self.silent
    }

    /// Runs the stage over a block, in place, and says what it silenced and
    /// clipped.
    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) -> OutputCounts {
        let mut counts = OutputCounts::default();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            counts.silenced += u64::from(!l.is_finite()) + u64::from(!r.is_finite());
            let (mut a, mut b) = (sanitize(*l), sanitize(*r));
            if self.mode != OutputSafety::Off {
                a = self.dc[0].tick(a);
                b = self.dc[1].tick(b);
                (a, b) = self.limiter.tick(a, b);
            }
            let gain = self.fade.tick();
            let (a, b) = (a * gain, b * gain);
            counts.clipped += u64::from(a.abs() > 1.0) + u64::from(b.abs() > 1.0);
            *l = a.clamp(-1.0, 1.0);
            *r = b.clamp(-1.0, 1.0);
            self.silent = self.stopping && gain == 0.0;
            if gain == 0.0
                && let Some(mode) = self.pending.take()
            {
                self.mode = mode;
                self.dc.iter_mut().for_each(DcBlocker::reset);
                self.limiter.set_mode(limiter_mode(mode));
                self.fade_in();
            }
        }
        counts
    }
}

/// What the stage did to one block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OutputCounts {
    /// Samples that were NaN or infinity, now 0.
    pub silenced: u64,
    /// Samples beyond -1 to 1, now at the limit.
    pub clipped: u64,
}

/// NaN and infinity become 0 (ADR-0007). Other values are clamped to
/// [`INPUT_LIMIT`].
fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
    } else {
        0.0
    }
}

/// The stage's DC blocker (rule 5).
fn dc_blocker(sample_rate: f32) -> DcBlocker {
    DcBlocker::new(DC_CUTOFF_HZ, sample_rate)
}

/// The stage's limiter (rule 4).
fn limiter(sample_rate: f32) -> Limiter {
    Limiter::new(limiter::Settings {
        ceiling: CEILING,
        lookahead: samples(LOOKAHEAD_SECONDS, sample_rate).max(1),
        release_seconds: RELEASE_SECONDS,
        sample_rate,
    })
}

/// The limiter's mode for a safety mode. Off does not run the limiter.
fn limiter_mode(mode: OutputSafety) -> limiter::Mode {
    if mode == OutputSafety::Clean {
        limiter::Mode::Lookahead
    } else {
        limiter::Mode::Direct
    }
}

/// A linear fade between 0 and 1.
struct Fade {
    position: usize,
    length: usize,
    rising: bool,
    /// Samples to stay at 0 before rising.
    hold: usize,
}

impl Fade {
    fn new(sample_rate: f32) -> Self {
        Self {
            position: 0,
            length: samples(FADE_SECONDS, sample_rate).max(1),
            rising: true,
            hold: 0,
        }
    }

    /// The gain for this sample, then one step on.
    fn tick(&mut self) -> f32 {
        if self.hold > 0 {
            self.hold -= 1;
            return 0.0;
        }
        let gain = self.position as f32 / self.length as f32;
        if self.rising {
            self.position = (self.position + 1).min(self.length);
        } else {
            self.position = self.position.saturating_sub(1);
        }
        gain
    }
}

#[cfg(test)]
mod tests;
