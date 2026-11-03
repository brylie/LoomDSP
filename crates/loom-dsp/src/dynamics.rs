//! A feed-forward dynamics processor: compress, limit, expand, gate, or duck.
//!
//! A detector turns a signal into a level in dB. A static curve maps the level
//! to a target gain. The target is then smoothed in dB, with attack and release
//! times. Smoothing after the curve keeps the threshold and the ratio where they
//! are set. It returns one gain, so a caller can use it on any number of
//! channels.
//!
//! Source: D. Giannoulis, M. Massberg, and J. D. Reiss, "Digital Dynamic Range
//! Compressor Design: A Tutorial and Analysis", JAES 60, 2012. The two
//! thresholds and the hold on a gate are built from first principles.
//!
//! ```
//! use loom_dsp::dynamics::{Coefficients, Curve, Detection, Dynamics, Mode, Settings};
//!
//! let coefficients = Coefficients::new(Settings {
//!     curve: Curve {
//!         mode: Mode::Compress,
//!         threshold_db: -18.0,
//!         ratio: 4.0,
//!         knee_db: 6.0,
//!         range_db: 40.0,
//!     },
//!     detection: Detection::Peak,
//!     attack_seconds: 0.01,
//!     release_seconds: 0.15,
//!     sample_rate: 48_000.0,
//! });
//! let mut dynamics = Dynamics::new(48_000.0);
//! let gain = dynamics.tick(0.5, coefficients);
//! assert!(gain <= 1.0);
//! ```

/// How far below the threshold a gate or duck must fall to change back.
const HYSTERESIS_DB: f32 = 3.0;

/// How long the level must stay there first.
const HOLD_SECONDS: f32 = 0.02;

/// The time constant of the RMS detector's power filter.
const RMS_SECONDS: f32 = 0.01;

/// The level that reads as silence. The logarithm of 0 is not finite.
const FLOOR_DB: f32 = -120.0;

/// The most power the RMS detector takes in one sample: 380 dB, far above any
/// real signal, and small enough that the filter's sums stay finite.
const MOST_POWER: f32 = 1e38;

/// A gain this close to its target has arrived. Without this, a gain falling
/// toward 0 dB would become a denormal.
const SETTLED_DB: f64 = 1e-6;

/// The per-sample factor of a one-pole with this time constant.
#[inline]
fn pole(seconds: f32, sample_rate: f32) -> f32 {
    (-1.0 / (seconds * sample_rate)).exp()
}

/// What the gain does about the level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Turns down what is above the threshold, by the ratio.
    Compress,
    /// Keeps the signal at or under the threshold.
    Limit,
    /// Turns down what is below the threshold, by the ratio.
    Expand,
    /// Mutes what is below the threshold.
    Gate,
    /// Turns the signal down while the level is above the threshold.
    Duck,
}

/// How the level is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// The absolute value of each sample.
    Peak,
    /// The square root of the power, averaged over 10 ms.
    Rms,
}

/// The static curve, from a level to a target gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    /// What the curve does.
    pub mode: Mode,
    /// Where it acts, in dB.
    pub threshold_db: f32,
    /// At least 1. Used by compress and expand.
    pub ratio: f32,
    /// The width of the rounded corner, at least 0. Used by compress and expand.
    pub knee_db: f32,
    /// The most the gain falls, at least 0. Used by expand, gate, and duck.
    pub range_db: f32,
}

impl Curve {
    /// The target gain in dB for a level in dB. A panel can draw it, and
    /// [`Dynamics`] calls it. A gate and a duck here have one threshold:
    /// `Dynamics` adds the second one and the hold.
    #[inline]
    pub fn gain_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        match self.mode {
            Mode::Compress => {
                let slope = 1.0 / self.ratio - 1.0;
                if 2.0 * over <= -self.knee_db {
                    0.0
                } else if 2.0 * over >= self.knee_db {
                    slope * over
                } else {
                    let into = over + self.knee_db / 2.0;
                    slope * into * into / (2.0 * self.knee_db)
                }
            }
            Mode::Limit => (-over).min(0.0),
            Mode::Expand => {
                let slope = self.ratio - 1.0;
                let gain = if 2.0 * over >= self.knee_db {
                    0.0
                } else if 2.0 * over <= -self.knee_db {
                    slope * over
                } else {
                    let into = over - self.knee_db / 2.0;
                    -slope * into * into / (2.0 * self.knee_db)
                };
                gain.max(-self.range_db)
            }
            Mode::Gate => {
                if over >= 0.0 {
                    0.0
                } else {
                    -self.range_db
                }
            }
            Mode::Duck => {
                if over >= 0.0 {
                    -self.range_db
                } else {
                    0.0
                }
            }
        }
    }
}

/// What sets a [`Dynamics`] for one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// The static curve.
    pub curve: Curve,
    /// How the level is read. A limiter always reads peaks.
    pub detection: Detection,
    /// How fast the gain moves when the signal rises, above 0. A limiter acts
    /// on the same sample.
    pub attack_seconds: f32,
    /// How fast the gain moves when the signal falls, above 0.
    pub release_seconds: f32,
    /// The sample rate in hertz.
    pub sample_rate: f32,
}

/// The settings for one sample. Computing them costs two exponentials, so a
/// caller with fixed settings computes them once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    curve: Curve,
    detection: Detection,
    attack: f32,
    release: f32,
}

impl Coefficients {
    /// Computes the settings.
    #[inline]
    pub fn new(settings: Settings) -> Self {
        let Settings {
            curve,
            detection,
            attack_seconds,
            release_seconds,
            sample_rate,
        } = settings;
        debug_assert!(curve.ratio >= 1.0, "ratio {}", curve.ratio);
        debug_assert!(curve.knee_db >= 0.0, "knee {}", curve.knee_db);
        debug_assert!(curve.range_db >= 0.0, "range {}", curve.range_db);
        debug_assert!(attack_seconds > 0.0, "attack {attack_seconds}");
        debug_assert!(release_seconds > 0.0, "release {release_seconds}");
        let limit = curve.mode == Mode::Limit;
        Self {
            curve,
            detection: if limit { Detection::Peak } else { detection },
            attack: if limit { 0.0 } else { pole(attack_seconds, sample_rate) },
            release: pole(release_seconds, sample_rate),
        }
    }
}

/// The running state of one dynamics processor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dynamics {
    /// The RMS detector's factor.
    rms_pole: f32,
    /// How many frames the hold lasts.
    hold_frames: u32,
    /// The RMS detector's power. It always runs, so a change of detection finds
    /// it settled.
    power: f32,
    /// The smoothed gain in dB. It is an `f64`, because in an `f32` a slow
    /// release stalls: near -40 dB, a step of 1e-5 of a small distance rounds
    /// away, and the gain stops 0.2 dB short of its target.
    gain_db: f64,
    /// Whether a gate or a duck sees the level as above its threshold.
    above: bool,
    /// Frames left before `above` can fall.
    hold: u32,
}

impl Dynamics {
    /// Creates a processor at unity gain.
    #[inline]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            rms_pole: pole(RMS_SECONDS, sample_rate),
            hold_frames: (HOLD_SECONDS * sample_rate) as u32,
            power: 0.0,
            gain_db: 0.0,
            above: false,
            hold: 0,
        }
    }

    /// Back to unity gain, with an empty detector.
    #[inline]
    pub fn reset(&mut self) {
        self.power = 0.0;
        self.gain_db = 0.0;
        self.above = false;
        self.hold = 0;
    }

    /// The smoothed gain in dB, at most 0.
    #[inline]
    pub fn gain_db(&self) -> f32 {
        self.gain_db as f32
    }

    /// The level in dB for one sample of the detector's signal.
    #[inline]
    fn detect(&mut self, key: f32, detection: Detection) -> f32 {
        let peak = key.abs();
        // A finite sample can still overflow when squared, and an infinite
        // power would stay in the filter.
        let squared = (peak * peak).min(MOST_POWER);
        self.power = squared + self.rms_pole * (self.power - squared);
        // The power of a falling signal would become a denormal.
        if self.power < 1e-30 {
            self.power = 0.0;
        }
        let level_db = match detection {
            Detection::Peak if peak > 0.0 => 20.0 * peak.log10(),
            Detection::Rms if self.power > 0.0 => 10.0 * self.power.log10(),
            _ => FLOOR_DB,
        };
        level_db.max(FLOOR_DB)
    }

    /// Moves the two-threshold state of a gate or a duck on by one frame.
    #[inline]
    fn update_above(&mut self, over_db: f32) {
        if over_db >= 0.0 {
            self.above = true;
            self.hold = self.hold_frames;
        } else if over_db < -HYSTERESIS_DB {
            if self.hold == 0 {
                self.above = false;
            } else {
                self.hold -= 1;
            }
        } else {
            self.hold = self.hold_frames;
        }
    }

    /// Takes one sample of the detector's signal, and returns the gain to apply
    /// as a factor. `key` must be finite. For several channels, pass the one
    /// with the largest absolute value, so they all get one gain.
    #[inline]
    pub fn tick(&mut self, key: f32, coefficients: Coefficients) -> f32 {
        debug_assert!(key.is_finite(), "key {key}");
        let Coefficients {
            curve,
            detection,
            attack,
            release,
        } = coefficients;
        let level_db = self.detect(key, detection);
        let target = match curve.mode {
            Mode::Gate => {
                self.update_above(level_db - curve.threshold_db);
                if self.above { 0.0 } else { -curve.range_db }
            }
            Mode::Duck => {
                self.update_above(level_db - curve.threshold_db);
                if self.above { -curve.range_db } else { 0.0 }
            }
            _ => curve.gain_db(level_db),
        };
        // Attack means the signal rose, in every mode. A compressor's gain
        // falls then, and a gate's rises.
        let signal_rose = match curve.mode {
            Mode::Expand | Mode::Gate => f64::from(target) > self.gain_db,
            Mode::Compress | Mode::Limit | Mode::Duck => f64::from(target) < self.gain_db,
        };
        let factor = if signal_rose { attack } else { release };
        let target = f64::from(target);
        self.gain_db = target + f64::from(factor) * (self.gain_db - target);
        if (self.gain_db - target).abs() < SETTLED_DB {
            self.gain_db = target;
        }
        (self.gain_db() * std::f32::consts::LN_10 / 20.0).exp()
    }
}

#[cfg(test)]
mod tests;
