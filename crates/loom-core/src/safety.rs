//! The safety stage on the engine's final output (ADR-0019).
//!
//! Spec: `docs/design/output-safety.md`. Why each step exists:
//! `docs/learn/output-safety.md`.
//!
//! Every step advances one sample at a time, so the output is the same for
//! any block size. All memory is allocated when the stage is built, so
//! nothing here allocates on the audio thread, not even a mode change.

use std::collections::VecDeque;

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
            dc: [DcBlocker::new(sample_rate), DcBlocker::new(sample_rate)],
            limiter: Limiter::new(sample_rate),
            fade: Fade::new(sample_rate),
        };
        stage.fade_in();
        stage
    }

    /// Starts the fade-in. In Clean mode the delay line is still silent, so
    /// the fade waits until the first real sample comes out of it. A fade
    /// over silence would meet that sample part way up, as a step.
    fn fade_in(&mut self) {
        self.fade.rising = true;
        self.fade.hold = if self.mode == OutputSafety::Clean {
            self.limiter.lookahead
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
        self.limiter.reset();
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

    /// Runs the stage over a block, in place.
    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (mut a, mut b) = (sanitize(*l), sanitize(*r));
            if self.mode != OutputSafety::Off {
                a = self.dc[0].tick(a);
                b = self.dc[1].tick(b);
                (a, b) = self.limiter.tick(a, b, self.mode == OutputSafety::Clean);
            }
            let gain = self.fade.tick();
            *l = (a * gain).clamp(-1.0, 1.0);
            *r = (b * gain).clamp(-1.0, 1.0);
            self.silent = self.stopping && gain == 0.0;
            if gain == 0.0
                && let Some(mode) = self.pending.take()
            {
                self.mode = mode;
                self.dc.iter_mut().for_each(DcBlocker::reset);
                self.limiter.reset();
                self.fade_in();
            }
        }
    }
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

/// A first-order high-pass filter: `y[n] = a * (y[n-1] + x[n] - x[n-1])`.
struct DcBlocker {
    coefficient: f32,
    previous_in: f32,
    previous_out: f32,
}

impl DcBlocker {
    fn new(sample_rate: f32) -> Self {
        Self {
            coefficient: (-std::f32::consts::TAU * DC_CUTOFF_HZ / sample_rate).exp(),
            previous_in: 0.0,
            previous_out: 0.0,
        }
    }

    fn tick(&mut self, input: f32) -> f32 {
        // The difference first: added to a large held input, a small output
        // would round away and stop decaying.
        let mut output = self.coefficient * (self.previous_out + (input - self.previous_in));
        // A held offset decays toward 0 for ever. Stop it before it becomes
        // a denormal, which is slow on some CPUs.
        if output.abs() < 1e-20 {
            output = 0.0;
        }
        self.previous_in = input;
        self.previous_out = output;
        output
    }

    fn reset(&mut self) {
        self.previous_in = 0.0;
        self.previous_out = 0.0;
    }
}

/// A stereo-linked peak limiter. One gain for both channels, so the stereo
/// image does not move while it acts.
///
/// Without lookahead the gain falls in the sample that goes over. With
/// lookahead the audio is delayed by `lookahead` samples, and the gain is
/// the average, over the last `lookahead + 1` samples, of the lowest gain
/// needed anywhere in the window ahead. Every value in that average is at or
/// under the gain the delayed sample needs, so the average is too, and it
/// changes in straight lines rather than steps.
struct Limiter {
    /// Gain applied to the last sample, after release.
    gain: f32,
    /// Per-sample factor that moves the gain toward 1.
    release: f32,
    lookahead: usize,
    /// Delayed audio, `lookahead` frames.
    delay: VecDeque<(f32, f32)>,
    /// Sliding minimum of the needed gain: (sample number, gain), with gains
    /// rising from front to back.
    minimum: VecDeque<(u64, f32)>,
    /// The last `lookahead + 1` window minimums, and their sum.
    window: VecDeque<f32>,
    window_sum: f64,
    /// Samples seen since the last reset.
    count: u64,
}

impl Limiter {
    fn new(sample_rate: f32) -> Self {
        let lookahead = samples(LOOKAHEAD_SECONDS, sample_rate).max(1);
        let mut limiter = Self {
            gain: 1.0,
            release: (-1.0 / (RELEASE_SECONDS * sample_rate)).exp(),
            lookahead,
            delay: VecDeque::with_capacity(lookahead + 1),
            minimum: VecDeque::with_capacity(lookahead + 2),
            window: VecDeque::with_capacity(lookahead + 2),
            window_sum: 0.0,
            count: 0,
        };
        limiter.reset();
        limiter
    }

    /// Back to unity gain and silence in the delay line. Keeps capacity, so
    /// it never allocates.
    fn reset(&mut self) {
        self.gain = 1.0;
        self.delay.clear();
        self.delay.extend(std::iter::repeat_n((0.0, 0.0), self.lookahead));
        self.minimum.clear();
        self.window.clear();
        self.window.extend(std::iter::repeat_n(1.0, self.lookahead + 1));
        self.window_sum = (self.lookahead + 1) as f64;
        self.count = 0;
    }

    fn tick(&mut self, left: f32, right: f32, lookahead: bool) -> (f32, f32) {
        let peak = left.abs().max(right.abs());
        let needed = if peak > CEILING { CEILING / peak } else { 1.0 };
        let (target, left, right) = if lookahead {
            self.look_ahead(needed, left, right)
        } else {
            (needed, left, right)
        };
        let released = 1.0 - (1.0 - self.gain) * self.release;
        self.gain = target.min(released);
        // The clamp only trims rounding: `x * (CEILING / x)` can land one
        // step above the ceiling.
        (
            (left * self.gain).clamp(-CEILING, CEILING),
            (right * self.gain).clamp(-CEILING, CEILING),
        )
    }

    /// Pushes one sample in, and returns the gain for the sample coming out
    /// of the delay line, with that sample.
    fn look_ahead(&mut self, needed: f32, left: f32, right: f32) -> (f32, f32, f32) {
        let n = self.count;
        self.count += 1;
        while self.minimum.back().is_some_and(|&(_, gain)| gain >= needed) {
            self.minimum.pop_back();
        }
        self.minimum.push_back((n, needed));
        let oldest = n.saturating_sub(self.lookahead as u64);
        while self.minimum.front().is_some_and(|&(i, _)| i < oldest) {
            self.minimum.pop_front();
        }
        let lowest = self.minimum.front().map_or(1.0, |&(_, gain)| gain);

        self.window.push_back(lowest);
        self.window_sum += f64::from(lowest);
        if let Some(old) = self.window.pop_front() {
            self.window_sum -= f64::from(old);
        }
        let average = (self.window_sum / (self.lookahead + 1) as f64) as f32;

        self.delay.push_back((left, right));
        let (left, right) = self.delay.pop_front().unwrap_or((0.0, 0.0));
        (average.min(1.0), left, right)
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
