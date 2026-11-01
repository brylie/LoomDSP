//! Parameter smoothing.
//!
//! Why: when a knob jumps from one value to another, applying the new value
//! instantly causes an audible click or "zipper" noise. Smoothing moves the
//! value toward its target over a few milliseconds.

/// A one-pole smoothed value.
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    target: f32,
    /// Remaining distance from the target. Storing the distance (rather than
    /// the current value) lets it decay all the way to zero; computing
    /// `target + (current - target) * c` can stall one rounding step short.
    distance: f32,
    coefficient: f32,
}

impl Smoothed {
    /// Creates a smoothed value that settles in roughly `time_seconds`.
    pub fn new(initial: f32, sample_rate: f32, time_seconds: f32) -> Self {
        let mut smoothed = Self {
            target: initial,
            distance: 0.0,
            coefficient: 0.0,
        };
        smoothed.set_time(sample_rate, time_seconds);
        smoothed
    }

    /// Changes the smoothing time.
    pub fn set_time(&mut self, sample_rate: f32, time_seconds: f32) {
        let samples = (time_seconds * sample_rate).max(1.0);
        // One-pole coefficient: reaches about 99% of the target after
        // `samples` steps (time constant = samples / 5).
        self.coefficient = (-5.0 / samples).exp();
    }

    /// Sets a new target value.
    #[inline]
    pub fn set_target(&mut self, target: f32) {
        self.distance += self.target - target;
        self.target = target;
    }

    /// Jumps straight to a value with no smoothing.
    #[inline]
    pub fn reset(&mut self, value: f32) {
        self.target = value;
        self.distance = 0.0;
    }

    /// The value the smoother is moving toward.
    #[inline]
    pub fn target(&self) -> f32 {
        self.target
    }

    /// The current value, without advancing.
    #[inline]
    pub fn current(&self) -> f32 {
        self.target + self.distance
    }

    /// The current value while it is still moving toward the target.
    /// `None` once it has settled.
    #[inline]
    pub fn gliding(&self) -> Option<f32> {
        (self.distance != 0.0).then(|| self.current())
    }

    /// Advances one sample and returns the new value.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.distance *= self.coefficient;
        if self.distance.abs() < 1e-9 {
            // Snap to exactly zero, which also avoids denormals.
            self.distance = 0.0;
        }
        self.target + self.distance
    }
}

#[cfg(test)]
mod tests;
