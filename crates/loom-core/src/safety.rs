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
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn run(stage: &mut SafetyStage, input: &[f32]) -> Vec<f32> {
        let mut left = input.to_vec();
        let mut right = input.to_vec();
        stage.process(&mut left, &mut right);
        assert_eq!(left, right);
        left
    }

    #[test]
    fn modes_read_back_from_their_names() {
        for mode in OutputSafety::ALL {
            assert_eq!(mode.name().parse(), Ok(mode));
        }
        assert!("loud".parse::<OutputSafety>().is_err());
    }

    #[test]
    fn off_only_sanitizes_and_clips_after_the_fade() {
        let mut stage = SafetyStage::new(OutputSafety::Off, RATE);
        run(&mut stage, &vec![0.0; 1000]);
        let input = [0.25, -0.5, 1.5, -7.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e38];
        assert_eq!(run(&mut stage, &input), [0.25, -0.5, 1.0, -1.0, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn the_fade_starts_at_zero_and_rises_in_a_straight_line() {
        let mut stage = SafetyStage::new(OutputSafety::Off, RATE);
        let out = run(&mut stage, &vec![0.5; 1000]);
        let length = samples(FADE_SECONDS, RATE);
        assert_eq!(out[0], 0.0);
        assert!((out[length / 2] - 0.25).abs() < 1e-6);
        assert_eq!(out[length], 0.5);
        assert!(out.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn clean_mode_ramps_the_gain_down_before_a_peak() {
        let mut limiter = Limiter::new(RATE);
        let lookahead = limiter.lookahead;
        let input: Vec<f32> = (0..400).map(|i| if i < 200 { 0.5 } else { 2.0 }).collect();
        let out: Vec<f32> = input.iter().map(|&x| limiter.tick(x, x, true).0).collect();
        assert!(out[..lookahead].iter().all(|&s| s == 0.0));
        assert!(out.iter().all(|s| s.abs() <= CEILING));
        // The quiet samples just before the step come out turned down a
        // little more each time: a ramp, not a step.
        let ramp = &out[199..200 + lookahead];
        assert_eq!(ramp[0], 0.5);
        assert!(ramp.windows(2).all(|w| w[1] < w[0]));
        assert!(ramp[lookahead] < 0.25);
        // The loud sample comes out at the ceiling.
        assert!((out[200 + lookahead] - CEILING).abs() < 1e-5);
    }

    #[test]
    fn low_latency_mode_turns_down_the_sample_that_goes_over() {
        let mut limiter = Limiter::new(RATE);
        assert_eq!(limiter.tick(0.5, -0.25, false), (0.5, -0.25));
        assert_eq!(limiter.tick(2.0, -1.0, false), (CEILING, -CEILING / 2.0));
        // Then it recovers slowly.
        let (next, _) = limiter.tick(0.5, 0.0, false);
        assert!(next < 0.25);
    }

    #[test]
    fn a_large_held_offset_decays_to_zero() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let mut dc = DcBlocker::new(sample_rate);
            let mut output = 0.0;
            for _ in 0..sample_rate as usize {
                output = dc.tick(INPUT_LIMIT);
            }
            // About e^-31 of the step after 1 s. Rounding once held it near
            // 0.0015 at 48 kHz.
            assert!(output.abs() < 1e-9, "{sample_rate} Hz: {output}");
        }
    }

    #[test]
    fn huge_values_do_not_stick() {
        for mode in [OutputSafety::LowLatency, OutputSafety::Clean] {
            let mut stage = SafetyStage::new(mode, RATE);
            run(&mut stage, &[1e38, f32::INFINITY, f32::NAN, 1e38, -1e38]);
            let sine: Vec<f32> = (0..48_000)
                .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / RATE).sin())
                .collect();
            let out = run(&mut stage, &sine);
            let peak = out[24_000..].iter().fold(0.0f32, |p, s| p.max(s.abs()));
            assert!(peak > 0.49 && peak <= 0.5, "{mode:?}: {peak}");
        }
    }

    #[test]
    fn a_mode_change_dips_to_silence_and_back() {
        let mut stage = SafetyStage::new(OutputSafety::LowLatency, RATE);
        let length = samples(FADE_SECONDS, RATE);
        run(&mut stage, &vec![0.5; 2 * length]);
        stage.set_mode(OutputSafety::Clean);
        // Asking again for the mode on its way changes nothing.
        stage.set_mode(OutputSafety::Clean);
        let out = run(&mut stage, &vec![0.5; 4 * length]);
        let silent = out.iter().position(|&s| s == 0.0).unwrap();
        assert!((length..length + 2).contains(&silent), "{silent}");
        assert_eq!(stage.mode, OutputSafety::Clean);
        assert!(stage.pending.is_none());
        // No jump bigger than one fade step, plus the DC blocker's drift.
        let step = 0.5 / length as f32 + 1e-3;
        assert!(out.windows(2).all(|w| (w[1] - w[0]).abs() <= step));
    }

    #[test]
    fn a_fade_out_during_a_dip_goes_down_and_stays_down() {
        let mut stage = SafetyStage::new(OutputSafety::Off, RATE);
        let length = samples(FADE_SECONDS, RATE);
        run(&mut stage, &vec![0.5; 2 * length]);
        stage.set_mode(OutputSafety::Clean);
        run(&mut stage, &vec![0.5; length / 2]);
        stage.fade_out();
        // A mode asked for while stopping changes nothing.
        stage.set_mode(OutputSafety::LowLatency);
        let out = run(&mut stage, &vec![0.5; 2 * length]);
        assert!(out.windows(2).all(|w| w[1] <= w[0]), "only goes down");
        let silent = out.iter().position(|&s| s == 0.0).unwrap();
        assert!(silent <= length / 2 + 1, "{silent}");
        assert!(out[silent..].iter().all(|&s| s == 0.0), "and stays at 0");
        assert!(stage.is_silent());
        assert_eq!(stage.mode, OutputSafety::Off);
    }

    #[test]
    fn a_fade_out_while_clean_waits_for_its_first_sample_is_silent_at_once() {
        let mut stage = SafetyStage::new(OutputSafety::Clean, RATE);
        stage.fade_out();
        assert!(!stage.is_silent(), "nothing played yet");
        let out = run(&mut stage, &[0.5; 4]);
        assert_eq!(out, [0.0; 4]);
        assert!(stage.is_silent());
    }
}
