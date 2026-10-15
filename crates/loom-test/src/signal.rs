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
