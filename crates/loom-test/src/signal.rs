//! Input signals for a test render.

/// A signal connected to one input of the module under test.
pub struct Signal(Source);

enum Source {
    Constant(f32),
    Samples(Vec<f32>),
    Fn(Box<dyn Fn(usize) -> f32>),
    Phase(f64),
    Gate { on: f64, off: f64 },
}

impl Signal {
    /// The same value at every frame.
    pub fn constant(value: f32) -> Self {
        Self(Source::Constant(value))
    }

    /// These samples. The render fails if there are fewer than its length.
    pub fn samples(samples: impl Into<Vec<f32>>) -> Self {
        Self(Source::Samples(samples.into()))
    }

    /// `f(frame)` for each frame, counted from the start of the render.
    pub fn from_fn(f: impl Fn(usize) -> f32 + 'static) -> Self {
        Self(Source::Fn(Box::new(f)))
    }

    /// A phase ramp from 0 up to 1 at `hz`. Each value is computed in `f64`
    /// from the frame number, so it does not drift.
    pub fn phase(hz: f64) -> Self {
        Self(Source::Phase(hz))
    }

    /// 1.0 from `on` seconds up to `off` seconds, 0.0 elsewhere.
    pub fn gate(on: f64, off: f64) -> Self {
        Self(Source::Gate { on, off })
    }

    /// A 50 Hz wave from -0.75 to 0.75, for `Harness::check_non_finite_recovery`.
    /// On a gate input it crosses the 0.5 threshold, at 48 kHz on frames 112,
    /// 1072, and 2032.
    ///
    /// The second rise falls inside the check's bad frames, so a module that
    /// stores an input on a trigger on another input sees the bad value. The
    /// third comes after the recovery time, so it cannot hide a kept bad value.
    pub fn non_finite_busy() -> Self {
        Self::from_fn(|frame| 0.75 * (frame as f64 * std::f64::consts::TAU * 50.0 / 48_000.0).sin() as f32)
    }

    /// A 2 Hz wave from -0.75 to 0.75, for `Harness::check_extreme_recovery`. On
    /// a gate input it crosses the 0.5 threshold, at 48 kHz on frames 2788 and
    /// 26788.
    ///
    /// It is slow on purpose. The next rising edge overwrites bad state that
    /// the first one captured, so it must come after the comparison. At an
    /// audio rate it comes first, and the check passes a module that keeps a
    /// bad value for a whole cycle.
    pub fn extreme_busy() -> Self {
        Self::from_fn(|frame| 0.75 * (frame as f64 * std::f64::consts::TAU * 2.0 / 48_000.0).sin() as f32)
    }

    /// The signal's first `frames` frames.
    #[track_caller]
    pub(crate) fn render(&self, frames: usize, sample_rate: f32) -> Vec<f32> {
        let sample_rate = f64::from(sample_rate);
        match &self.0 {
            Source::Constant(value) => vec![*value; frames],
            Source::Samples(samples) => {
                assert!(
                    samples.len() >= frames,
                    "Signal::samples has {} samples, but the render is {frames} frames long",
                    samples.len()
                );
                samples[..frames].to_vec()
            }
            Source::Fn(f) => (0..frames).map(f).collect(),
            Source::Phase(hz) => (0..frames)
                .map(|frame| {
                    let x = frame as f64 * hz / sample_rate;
                    let phase = (x - x.floor()) as f32;
                    if phase >= 1.0 { 0.0 } else { phase }
                })
                .collect(),
            Source::Gate { on, off } => (0..frames)
                .map(|frame| {
                    let t = frame as f64 / sample_rate;
                    if t >= *on && t < *off { 1.0 } else { 0.0 }
                })
                .collect(),
        }
    }
}
