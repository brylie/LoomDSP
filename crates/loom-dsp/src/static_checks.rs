//! Compile-time promises. A host moves an algorithm to its audio thread, so
//! each must be `Send`. A caller computes coefficients once and passes them
//! each sample, so each must be `Copy`. Adding a field that breaks either fails
//! the build here.

use crate::{delay, dynamics, envelope, filter, limiter, loudness, noise, oscillator, phase, random, slew};

const fn is_send<T: Send>() {}
const fn is_copy<T: Copy>() {}

const _: () = {
    is_send::<delay::Allpass>();
    is_send::<delay::DelayLine>();
    is_send::<dynamics::Dynamics>();
    is_send::<envelope::Envelope>();
    is_send::<filter::crossover::Crossover>();
    is_send::<filter::dc_blocker::DcBlocker>();
    is_send::<filter::eq::Cut>();
    is_send::<filter::eq::Section>();
    is_send::<filter::ladder::Ladder>();
    is_send::<filter::one_pole::OnePole>();
    is_send::<filter::svf::Svf>();
    is_send::<limiter::Limiter>();
    is_send::<loudness::LevelMatch>();
    is_send::<noise::Noise>();
    is_send::<oscillator::Pulse>();
    is_send::<oscillator::Saw>();
    is_send::<oscillator::Triangle>();
    is_send::<phase::Phasor>();
    is_send::<random::Rng>();
    is_send::<slew::Slew>();

    is_copy::<dynamics::Coefficients>();
    is_copy::<envelope::Settings>();
    is_copy::<filter::crossover::Coefficients>();
    is_copy::<filter::eq::Coefficients>();
    is_copy::<filter::eq::CutCoefficients>();
    is_copy::<filter::ladder::Coefficients>();
    is_copy::<filter::one_pole::Coefficients>();
    is_copy::<filter::svf::Coefficients>();
    is_copy::<random::Rng>();
};
