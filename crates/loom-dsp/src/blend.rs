//! Blending several signals into one: equal-power gains, the constant-power
//! pan law, the gain that keeps a sum of uncorrelated copies at the loudness
//! of one, the even spread of copies from -1 to 1, and the crossfade between
//! members of a select (`docs/design/sub-patches.md`, Modes).
//!
//! Two signals that are different sounds add in power, not in amplitude.
//! Gains on a quarter circle keep the sum of their squares at 1, so loudness
//! holds through a fade or across a pan, where linear gains dip by 3 dB in the
//! middle (C. Roads, *The Computer Music Tutorial*, MIT Press, 1996, on
//! crossfading and panning; U. Zölzer (ed.), *DAFX: Digital Audio Effects*,
//! 2nd ed., Wiley, 2011, on constant-power laws). The power of a sum of `n`
//! uncorrelated signals is the sum of their powers (A. Papoulis and
//! S. U. Pillai, *Probability, Random Variables and Stochastic Processes*,
//! 4th ed., McGraw-Hill, 2002, the variance of a sum of independent
//! variables), so scaling the sum by `1/√n` keeps the loudness of one.
//! Pitch and modulation are values, not sounds, so they fade with linear
//! gains that sum to 1, and a value both members share stays put. The
//! explainer is `docs/learn/unison.md`.
//!
//! ```
//! use loom_dsp::blend::{equal_power, MemberFade};
//!
//! let (a, b) = equal_power(0.5);
//! assert!((a * a + b * b - 1.0).abs() < 1e-6);
//!
//! // Ten samples of fade from member 1 to member 2.
//! let mut fade = MemberFade::new(10, 1);
//! let first = fade.tick(2);
//! assert_eq!((first.from, first.to), (1, 2));
//! assert_eq!(first.gain_from, 1.0);
//! ```

use core::f32::consts::{FRAC_PI_2, FRAC_PI_4};

/// The gains of an equal-power blend at `position` from 0 (all of the first
/// signal) to 1 (all of the second): `(cos θ, sin θ)` with `θ = position·π/2`.
/// The sum of their squares is 1. A position outside 0 to 1 is clamped, and
/// NaN gives the middle.
#[inline]
pub fn equal_power(position: f32) -> (f32, f32) {
    let position = if position.is_nan() {
        0.5
    } else {
        position.clamp(0.0, 1.0)
    };
    let (sin, cos) = (position * FRAC_PI_2).sin_cos();
    (cos, sin)
}

/// The gains of a linear blend at `position` from 0 to 1: `(1 - p, p)`.
/// They sum to 1, so a value both signals share passes through unchanged:
/// the law for pitch and modulation, which add in value, not in power. A
/// position outside 0 to 1 is clamped, and NaN gives the middle.
#[inline]
pub fn linear(position: f32) -> (f32, f32) {
    let position = if position.is_nan() {
        0.5
    } else {
        position.clamp(0.0, 1.0)
    };
    (1.0 - position, position)
}

/// How a fade shares two signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Law {
    /// Gains on a quarter circle, for audio: the power stays the same
    /// ([`equal_power`]).
    EqualPower,
    /// Gains that sum to 1, for pitch and modulation: a shared value stays
    /// the same ([`linear`]).
    Linear,
}

impl Law {
    /// The two gains at `position` from 0 to 1.
    #[inline]
    pub fn gains(self, position: f32) -> (f32, f32) {
        match self {
            Law::EqualPower => equal_power(position),
            Law::Linear => linear(position),
        }
    }
}

/// The left and right gains of the constant-power pan law at `offset` from -1
/// (left) to 1 (right): `θ = (offset + 1)·π/4`, left `cos θ`, right `sin θ`.
/// The centre gives `1/√2` on each side, -3 dB. An offset outside -1 to 1 is
/// clamped, and NaN gives the centre.
#[inline]
pub fn pan(offset: f32) -> (f32, f32) {
    let offset = if offset.is_nan() { 0.0 } else { offset.clamp(-1.0, 1.0) };
    let (sin, cos) = ((offset + 1.0) * FRAC_PI_4).sin_cos();
    (cos, sin)
}

/// The gain that keeps a sum of `copies` uncorrelated signals at the loudness
/// of one: `1/√copies`. Zero copies gives 1, so nothing divides by zero.
#[inline]
pub fn unison_gain(copies: u32) -> f32 {
    if copies <= 1 { 1.0 } else { 1.0 / (copies as f32).sqrt() }
}

/// The offset of copy `index` of `copies`, evenly spaced from -1 (the first)
/// to 1 (the last). One copy, or none, sits at 0. An index past the last is
/// held at 1.
#[inline]
pub fn spread_offset(index: u32, copies: u32) -> f32 {
    if copies <= 1 {
        return 0.0;
    }
    let index = index.min(copies - 1);
    -1.0 + 2.0 * index as f32 / (copies - 1) as f32
}

/// The two members a select plays on one frame, and their gains. When no
/// fade runs, `from` and `to` are the same member and `gain_to` is 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blend {
    /// The member fading out.
    pub from: usize,
    /// The member fading in, or the one playing.
    pub to: usize,
    /// The gain on `from`.
    pub gain_from: f32,
    /// The gain on `to`.
    pub gain_to: f32,
}

/// The crossfade between the members of a select, one step per sample, so it
/// gives the same result at any block size (`docs/design/sub-patches.md`, The
/// modules the flattener adds, Select).
///
/// - Not fading, and the target differs from the member playing: a fade
///   starts, equal power, `length` samples long. Its first frame is the old
///   member alone, and after `length` samples it is the new one alone.
/// - The target goes back to the member fading out: the fade reverses from
///   where it is. The gains on that frame do not change, because
///   `cos((π/2)(N-n)/N) = sin((π/2)n/N)`.
/// - The target goes to a third member during a fade: it waits until the fade
///   ends, then a new fade starts toward whatever the target is then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberFade {
    /// Samples a fade takes, at least 1.
    length: u32,
    /// The member fading out, or the one playing when not fading.
    from: usize,
    /// The member fading in, or the one playing.
    to: usize,
    /// Samples of the fade done so far; `length` when not fading.
    count: u32,
}

impl MemberFade {
    /// A select playing `member`, with fades `length` samples long.
    #[inline]
    pub fn new(length: u32, member: usize) -> Self {
        let length = length.max(1);
        Self {
            length,
            from: member,
            to: member,
            count: length,
        }
    }

    /// Back to playing `member` alone, with no fade.
    #[inline]
    pub fn reset(&mut self, member: usize) {
        *self = Self::new(self.length, member);
    }

    /// The member playing, or fading in.
    #[inline]
    pub fn member(&self) -> usize {
        self.to
    }

    /// True while a fade runs.
    #[inline]
    pub fn is_fading(&self) -> bool {
        self.count < self.length
    }

    /// One frame toward `target`, equal power: the members and gains for
    /// this frame.
    #[inline]
    pub fn tick(&mut self, target: usize) -> Blend {
        self.tick_with(target, Law::EqualPower)
    }

    /// One frame toward `target` with the gains of `law`. Either law keeps
    /// the turn back continuous: the gains at `N - n` are the gains at `n`
    /// swapped.
    #[inline]
    pub fn tick_with(&mut self, target: usize, law: Law) -> Blend {
        if self.is_fading() {
            if target == self.from {
                core::mem::swap(&mut self.from, &mut self.to);
                self.count = self.length - self.count;
            }
        } else if target != self.to {
            self.from = self.to;
            self.to = target;
            self.count = 0;
        }
        if !self.is_fading() {
            return Blend {
                from: self.to,
                to: self.to,
                gain_from: 0.0,
                gain_to: 1.0,
            };
        }
        let (gain_from, gain_to) = law.gains(self.count as f32 / self.length as f32);
        self.count += 1;
        Blend {
            from: self.from,
            to: self.to,
            gain_from,
            gain_to,
        }
    }

    /// One frame that switches to `target` at once, with no fade: for gate,
    /// trigger, and phase outputs (live-patching rule 9).
    #[inline]
    pub fn cut(&mut self, target: usize) -> Blend {
        self.reset(target);
        Blend {
            from: target,
            to: target,
            gain_from: 0.0,
            gain_to: 1.0,
        }
    }
}

#[cfg(test)]
mod tests;
