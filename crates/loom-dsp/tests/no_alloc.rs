//! A tick never allocates or frees, nor does `reset` or `copy_from` (ADR-0035).
//!
//! This test binary installs an allocator that aborts if any allocation or
//! deallocation happens inside `assert_no_alloc`. It is active in debug
//! builds, which is how `cargo test` runs. `assert_no_alloc` has no
//! `AllocDisabler` in release builds, so this file is empty there.

#![cfg(debug_assertions)]

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use loom_dsp::delay::{Allpass, DelayLine};
use loom_dsp::dynamics::{self, Dynamics};
use loom_dsp::envelope::{Envelope, Settings, power};
use loom_dsp::filter::dc_blocker::DcBlocker;
use loom_dsp::filter::{crossover, eq, ladder, one_pole, svf};
use loom_dsp::limiter::{self, Limiter, Mode};
use loom_dsp::loudness::LevelMatch;
use loom_dsp::noise::{Colour, Noise};
use loom_dsp::oscillator::{Pulse, Saw, SlowWave, Triangle};
use loom_dsp::slew::Slew;

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

const RATE: f32 = 48_000.0;
/// Enough frames to fill every queue and wrap every buffer in this file.
const FRAMES: usize = 1_000;

/// A test signal with falling peaks well over 1.
fn falling_peaks(i: usize) -> f32 {
    (100.0 - i as f32 * 0.05) * if i.is_multiple_of(2) { 1.0 } else { -1.0 }
}

/// The filters, including a slope change on the ladder.
#[test]
fn filters_never_allocate() {
    let mut state_variable = svf::Svf::new();
    let mut one_pole = one_pole::OnePole::new();
    let mut dc = DcBlocker::new(5.0, RATE);
    let mut ladder = ladder::Ladder::new(RATE);
    assert_no_alloc(|| {
        for i in 0..FRAMES {
            let x = falling_peaks(i);
            state_variable.tick(x, svf::Coefficients::new(1_000.0, 0.5, RATE));
            one_pole.tick(x, one_pole::Coefficients::new(1_000.0, RATE));
            dc.tick(x);
            if i == 10 {
                ladder.change_slope(ladder::Slope::Db12);
            }
            let feedback = ladder.step_slope();
            let settings = ladder::Settings {
                cutoff_hz: 1_000.0,
                feedback,
                input_gain: 1.0,
                sample_rate: RATE,
            };
            ladder.tick(x, ladder::Coefficients::new(settings));
        }
        state_variable.reset();
        one_pole.reset();
        dc.reset();
        ladder.reset(ladder::Slope::Db18);
        ladder.clear_loop();
    });
}

/// The oscillator shapes and the slow waves.
#[test]
fn shapes_never_allocate() {
    let (mut saw, mut pulse, mut triangle) = (Saw::new(), Pulse::new(), Triangle::new());
    assert_no_alloc(|| {
        for i in 0..FRAMES {
            let phase = (i as f32 * 0.013).fract();
            saw.tick(phase);
            pulse.tick(phase, 0.3);
            triangle.tick(phase);
            for wave in [
                SlowWave::Sine,
                SlowWave::Triangle,
                SlowWave::RampUp,
                SlowWave::RampDown,
                SlowWave::Square,
            ] {
                wave.value(f64::from(phase));
            }
        }
        saw.reset();
        pulse.reset(0.3);
        triangle.reset();
    });
}

/// Noise of each colour, with its restarts and state copy.
#[test]
fn noise_never_allocates() {
    let mut noise = Noise::new(RATE);
    let other = Noise::new(RATE);
    assert_no_alloc(|| {
        for _ in 0..FRAMES {
            for colour in [Colour::White, Colour::Pink, Colour::Brown] {
                noise.tick(colour);
            }
        }
        noise.restart(7);
        noise.restart_keeping_brown(8);
        noise.copy_brown_from(&other);
    });
}

/// The envelope through all its stages, and the slew limiter.
#[test]
fn envelope_and_slew_never_allocate() {
    let settings = Settings {
        attack: 0.001,
        decay: 0.001,
        sustain: 0.5,
        release: 0.001,
        ad: false,
        power: power(0.5),
    };
    let mut envelope = Envelope::new(RATE);
    let mut slew = Slew::new(RATE);
    assert_no_alloc(|| {
        for i in 0..FRAMES {
            envelope.tick(i < FRAMES / 2, settings);
            slew.tick(falling_peaks(i), 0.01, 0.01);
        }
        envelope.reset();
        slew.reset();
    });
}

/// The delay line and the all-pass, past the point where they wrap.
#[test]
fn delays_never_allocate() {
    let mut delay = DelayLine::new(0.002, RATE);
    let mut allpass = Allpass::new(0.002, RATE);
    let other = DelayLine::new(0.002, RATE);
    let other_allpass = Allpass::new(0.002, RATE);
    assert_no_alloc(|| {
        for i in 0..FRAMES {
            delay.tick(falling_peaks(i), 20.5);
            allpass.tick(falling_peaks(i), 20.5, 0.5);
        }
        assert!(delay.copy_from(&other));
        assert!(allpass.copy_from(&other_allpass));
        delay.reset();
        allpass.reset();
    });
}

/// The limiter in each mode. Falling peaks keep every sample in the sliding
/// minimum, which fills its queue to the most it can hold.
#[test]
fn the_limiter_never_allocates() {
    let mut limiter = Limiter::new(limiter::Settings {
        ceiling: 0.9,
        lookahead: 96,
        release_seconds: 0.1,
        sample_rate: RATE,
    });
    let mut copy = Limiter::new(limiter::Settings {
        ceiling: 0.9,
        lookahead: 96,
        release_seconds: 0.1,
        sample_rate: RATE,
    });
    assert_no_alloc(|| {
        for mode in [Mode::Lookahead, Mode::Direct] {
            limiter.set_mode(mode);
            for i in 0..FRAMES {
                limiter.tick(falling_peaks(i), falling_peaks(i));
            }
            // The copy takes queues at their fullest, then runs on from there.
            assert!(copy.copy_from(&limiter));
            copy.tick(1.0, 1.0);
            limiter.reset();
        }
    });
}

/// The equaliser sections and the crossover.
#[test]
fn eq_and_crossover_never_allocate() {
    let mut section = eq::Section::new();
    let mut cut = eq::Cut::new();
    let mut crossover = crossover::Crossover::new();
    assert_no_alloc(|| {
        for i in 0..FRAMES {
            let x = falling_peaks(i);
            let shape = eq::Shape::Bell {
                hz: 1_000.0,
                gain_db: 6.0,
                q: 1.0,
            };
            section.tick(x, eq::Coefficients::new(shape, RATE));
            cut.tick(x, eq::CutCoefficients::new(eq::CutSide::Low, 100.0, RATE));
            crossover.tick(x, crossover::Coefficients::new(200.0, 2_000.0, RATE));
        }
        section.reset();
        cut.reset();
        crossover.reset();
    });
}

/// Every mode of the dynamics processor, and the level match.
#[test]
fn dynamics_and_loudness_never_allocate() {
    let mut dynamics = Dynamics::new(RATE);
    let mut level_match = LevelMatch::new(RATE);
    assert_no_alloc(|| {
        for mode in [
            dynamics::Mode::Compress,
            dynamics::Mode::Limit,
            dynamics::Mode::Expand,
            dynamics::Mode::Gate,
            dynamics::Mode::Duck,
        ] {
            let coefficients = dynamics::Coefficients::new(dynamics::Settings {
                curve: dynamics::Curve {
                    mode,
                    threshold_db: -20.0,
                    ratio: 4.0,
                    knee_db: 6.0,
                    range_db: 40.0,
                },
                detection: dynamics::Detection::Rms,
                attack_seconds: 0.01,
                hold_seconds: 0.02,
                release_seconds: 0.1,
                sample_rate: RATE,
            });
            for i in 0..FRAMES {
                let x = falling_peaks(i);
                let gain = dynamics.tick(x, coefficients);
                level_match.tick([x, x], [x * gain, x * gain]);
            }
        }
        dynamics.reset();
        level_match.reset();
    });
}
