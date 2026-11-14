//! Click-free cable changes (live patching spec, rule 9; FR-GRAPH-05).
//!
//! When a schedule swap changes what feeds an input, the input holds the
//! last value it read and moves in a straight line to the new signal over
//! 5 ms, sample by sample. A modulation assignment that is added, removed,
//! or moved fades its contribution the same way.
//!
//! The compiler decides which inputs fade (`compile/fades.rs`) and gives
//! each one a fixed slot and buffer on the control thread. At the swap the
//! engine fills in the held values from the old schedule
//! ([`take_state_from`]), and each block it runs the ramps just before
//! their readers. Nothing here allocates, locks, or waits.
//!
//! Background: `docs/learn/hot-patching.md`.

use crate::compile::{ModSource, Schedule};
use crate::signal::hold_finite;

/// Module ID of the hidden Sum that a layer or a stack adds
/// (sub-patches.md, The modules the flattener adds).
pub const SUM_MODULE: &str = "loom.struct.sum";
/// Module ID of the hidden Stereo spread sum of a stack with spread.
pub const SPREAD_SUM_MODULE: &str = "loom.struct.spread_sum";
/// Module ID of the hidden Select of a select instance.
pub const SELECT_MODULE: &str = "loom.struct.select";
/// Module ID of the hidden Member of a select instance, whose value picks
/// what its Selects play.
pub const MEMBER_MODULE: &str = "loom.struct.member";
/// Module ID of the crossfade a slot swap adds (sub-patches.md, How a swap
/// crossfades).
pub const SWAP_FADE_MODULE: &str = "loom.struct.fade";

/// The mixers the editor adds. Rule 9 treats cables onto and off them
/// specially, because a mixer that plays one source alone carries that
/// source on with no step (live patching spec, rule 9, The mixers the
/// editor adds).
pub const MIXERS: [&str; 4] = [SUM_MODULE, SPREAD_SUM_MODULE, SELECT_MODULE, SWAP_FADE_MODULE];

/// How long a cable change fades. Fixed, with no setting (rule 9).
pub const CABLE_FADE_SECONDS: f32 = 0.005;

/// The fade length in frames: 5 ms, rounded, at least 1. The cast
/// saturates, so a NaN rate gives 1 and an infinite one `u32::MAX`. The
/// length never sizes an allocation: only `max_block` sizes buffers.
pub(crate) fn fade_frames(sample_rate: f32) -> u32 {
    ((sample_rate * CABLE_FADE_SECONDS).round() as u32).max(1)
}

/// A straight line from a held value to a signal, one step per sample.
///
/// Frame 0 gives the held value exactly. Frame `k` gives
/// `held · (1 − k/N) + signal · k/N`. From frame `N` on it gives the signal
/// unchanged. The position stops at `N` and never wraps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ramp {
    held: f32,
    position: u32,
    length: u32,
}

impl Ramp {
    /// A ramp from `held` over `length` frames. A held value that is NaN or
    /// infinite gives a finished ramp, so the input switches: holding it
    /// would only spread a broken value into a clean cable (rule 9).
    pub(crate) fn new(held: f32, length: u32) -> Self {
        let length = length.max(1);
        if held.is_finite() {
            Self {
                held,
                position: 0,
                length,
            }
        } else {
            Self::finished(length)
        }
    }

    /// A ramp that is already over: it passes the signal through.
    pub(crate) fn finished(length: u32) -> Self {
        let length = length.max(1);
        Self {
            held: 0.0,
            position: length,
            length,
        }
    }

    /// True once the ramp passes the signal through unchanged.
    pub(crate) fn is_done(&self) -> bool {
        self.position >= self.length
    }

    /// Ends the ramp at once, as a panic does.
    pub(crate) fn finish(&mut self) {
        self.position = self.length;
    }

    /// The value the ramp gives for `signal` at its current position,
    /// without moving on.
    #[inline]
    pub(crate) fn value(&self, signal: f32) -> f32 {
        if self.position >= self.length {
            signal
        } else if self.position == 0 {
            self.held
        } else {
            let t = self.position as f32 * (1.0 / self.length as f32);
            // The two-weight form cannot overflow for finite values of
            // opposite sign. `hold_finite` catches the one rounding step
            // that two same-sign values near the largest float could still
            // take past it (ADR-0037).
            hold_finite(self.held * (1.0 - t) + signal * t)
        }
    }

    /// The value for `signal`, then one step on.
    #[inline]
    pub(crate) fn next(&mut self, signal: f32) -> f32 {
        let value = self.value(signal);
        self.advance(1);
        value
    }

    /// Moves `frames` steps on, stopping at the end.
    #[inline]
    pub(crate) fn advance(&mut self, frames: u32) {
        self.position = self.position.saturating_add(frames).min(self.length);
    }

    /// Ramps `target` into `out`, one sample each.
    pub(crate) fn run(&mut self, target: &[f32], out: &mut [f32]) {
        if self.is_done() {
            out.copy_from_slice(target);
            return;
        }
        for (out, &signal) in out.iter_mut().zip(target) {
            *out = self.next(signal);
        }
    }

    /// The same ramp, continued from `other`'s held value and position.
    pub(crate) fn continue_from(&mut self, other: &Ramp) {
        self.held = other.held;
        self.position = other.position.min(self.length);
    }

    /// The held value, for a ramp that starts from another one's start.
    pub(crate) fn held(&self) -> f32 {
        self.held
    }
}

/// One input channel that fades in this schedule: a module input on one
/// copy, the right side of a stereo input, a bundle voice, a side of the
/// audio output, or a unit delay input.
///
/// The reader reads `buffer`. The fade writes it from `target`, the buffer
/// the reader would read with no fade, just before the reader runs. Only
/// `compile/build.rs` makes one, so the two pointers always name distinct
/// buffers of `max_block` floats owned by the same schedule, and no fade's
/// target is another fade's buffer.
pub(crate) struct InputFade {
    /// Input-channel key ([`input_key`]). Matches fades across swaps.
    pub(crate) key: u64,
    pub(crate) ramp: Ramp,
    /// True when the cable changed in this compile, so the fade starts from
    /// what the input read last. False for a fade carried from the last
    /// schedule, which goes on where it was or starts finished.
    pub(crate) fresh: bool,
    target: *const f32,
    buffer: *mut f32,
}

impl InputFade {
    /// A fade from `target` into `buffer`. The held value is set at the
    /// swap.
    pub(crate) fn new(key: u64, fresh: bool, length: u32, target: *const f32, buffer: *mut f32) -> Self {
        debug_assert!(!std::ptr::eq(target, buffer), "a fade reads another buffer");
        Self {
            key,
            ramp: Ramp::finished(length),
            fresh,
            target,
            buffer,
        }
    }

    /// The buffer the reader reads.
    #[cfg(test)]
    pub(crate) fn buffer(&self) -> *const f32 {
        self.buffer
    }

    /// The buffer the fade reads.
    pub(crate) fn target(&self) -> *const f32 {
        self.target
    }
}

/// The key of one channel of an input: module, port, and channel (voice
/// times 2 plus side). Sorted by module, then port, then channel.
pub(crate) fn input_key(module: u32, port: usize, channel: usize) -> u64 {
    debug_assert!(port < 1 << 16, "ports fit 16 bits");
    debug_assert!(channel < 1 << 16, "channels fit 16 bits");
    (u64::from(module) << 32) | ((port as u64) << 16) | channel as u64
}

/// The channel part of an input-channel key.
fn key_channel(key: u64) -> usize {
    (key & 0xFFFF) as usize
}

/// Runs `fades` over `len` frames from `offset`.
///
/// Runs on the audio thread, right before the fades' reader.
pub(crate) fn run(fades: &mut [InputFade], offset: usize, len: usize) {
    for fade in fades {
        // SAFETY: `compile/build.rs` makes every fade with `target` and
        // `buffer` pointing at two distinct buffers of `max_block` floats
        // owned by the running schedule, and no fade's target is another
        // fade's buffer, so nothing else holds either slice here. The
        // engine calls this with `offset + len <= frames <= max_block`.
        let (target, out) = unsafe {
            (
                std::slice::from_raw_parts(fade.target.add(offset), len),
                std::slice::from_raw_parts_mut(fade.buffer.add(offset), len),
            )
        };
        fade.ramp.run(target, out);
    }
}

/// True when every fade and contribution ramp of `schedule` has ended.
pub(crate) fn settled(schedule: &Schedule) -> bool {
    schedule.cable_fades.iter().all(|fade| fade.ramp.is_done())
        && schedule.fills.iter().all(|fill| {
            fill.sources.iter().all(|source| match source {
                ModSource::Live { ramp, .. } => ramp.is_none_or(|ramp| ramp.is_done()),
                ModSource::Leaving { ramp, .. } => ramp.is_done(),
            })
        })
}

/// Ends every fade and contribution ramp, as a panic does.
pub(crate) fn finish_all(schedule: &mut Schedule) {
    for fade in &mut schedule.cable_fades {
        fade.ramp.finish();
    }
    for fill in &mut schedule.fills {
        for source in &mut fill.sources {
            match source {
                ModSource::Live { ramp, .. } => {
                    if let Some(ramp) = ramp {
                        ramp.finish();
                    }
                }
                ModSource::Leaving { ramp, .. } => ramp.finish(),
            }
        }
    }
}

/// What an input read last in `old`, whose last block was `frames` frames.
/// Reads memory only when that block ran. When it did not, an input that
/// read a fixed default gives that value, and anything else gives `None`.
fn last_read(old: &Schedule, key: u64, frames: usize) -> Option<f32> {
    let i = old.reads.binary_search_by_key(&key, |read| read.key).ok()?;
    let read = &old.reads[i];
    match frames.checked_sub(1) {
        // SAFETY: every pointer in `reads` names a buffer of `max_block`
        // floats owned by `old`, which is alive here, and the last block
        // filled its first `frames <= max_block` floats.
        Some(last) => Some(unsafe { *read.ptr.add(last) }),
        None => read.constant,
    }
}

/// Fills in the held value of each fade and contribution ramp of `new` from
/// `old`, whose last block was `frames` frames long (0 when it never ran).
///
/// - A carried fade goes on with the old fade's held value and position, or
///   starts finished when the old one had none or had ended.
/// - A fresh fade starts from what the input read last. With no frame run,
///   an old fade gives its held value. A new copy with nothing to read
///   takes copy 0's start. Anything else starts from 0.
///
/// Copies values only, never pointers: `old` is sent back and freed after
/// the swap. Runs on the audio thread: no allocation, one merge over two
/// key-sorted indices and a binary search per fade.
pub(crate) fn take_state_from(new: &mut Schedule, old: &Schedule, frames: usize) {
    let mut j = 0;
    for &i in &new.cable_fade_order {
        let key = new.cable_fades[i as usize].key;
        while j < old.cable_fade_order.len() && old.cable_fades[old.cable_fade_order[j] as usize].key < key {
            j += 1;
        }
        let previous = old
            .cable_fade_order
            .get(j)
            .map(|&k| &old.cable_fades[k as usize])
            .filter(|previous| previous.key == key)
            .map(|previous| previous.ramp);
        let fade = &mut new.cable_fades[i as usize];
        let length = fade.ramp.length;
        fade.ramp = if fade.fresh {
            let held = if frames == 0 {
                previous
                    .map(|previous| previous.held())
                    .or_else(|| last_read(old, key, frames))
            } else {
                last_read(old, key, frames)
            };
            match held {
                Some(held) => Ramp::new(held, length),
                // Filled from copy 0 below, or 0.
                None => Ramp {
                    held: f32::NAN,
                    position: 0,
                    length,
                },
            }
        } else {
            match previous {
                Some(previous) if !previous.is_done() => {
                    let mut ramp = Ramp::finished(length);
                    ramp.continue_from(&previous);
                    ramp
                }
                _ => Ramp::finished(length),
            }
        };
    }
    // A fresh fade with nothing to read is a new copy, or a module that
    // read nothing: it takes copy 0's start (rule 6), or starts from 0.
    // Reads only fades already filled above.
    for n in 0..new.cable_fade_order.len() {
        let i = new.cable_fade_order[n] as usize;
        let fade = &new.cable_fades[i];
        if !(fade.fresh && fade.ramp.held.is_nan() && fade.ramp.position == 0) {
            continue;
        }
        let key = fade.key;
        let length = fade.ramp.length;
        let channel = key_channel(key);
        let copy0 = (channel != 0)
            .then(|| {
                let base = key & !0xFFFF;
                new.cable_fade_order
                    .binary_search_by_key(&base, |&k| new.cable_fades[k as usize].key)
                    .ok()
                    .map(|k| new.cable_fades[new.cable_fade_order[k] as usize].ramp)
            })
            .flatten()
            .filter(|ramp| !ramp.held.is_nan());
        new.cable_fades[i].ramp = match copy0 {
            Some(ramp) => {
                let mut copied = Ramp::finished(length);
                copied.continue_from(&ramp);
                copied
            }
            None => Ramp::new(0.0, length),
        };
    }
    take_contributions_from(new, old, frames);
}

/// Fills in the contribution ramps of `new`'s fills from `old`.
fn take_contributions_from(new: &mut Schedule, old: &Schedule, frames: usize) {
    let old_assignments = &old.assignments;
    for fill in &mut new.fills {
        let old_fill = old
            .fill_order
            .binary_search_by_key(&fill.key, |&k| old.fills[k as usize].key)
            .ok()
            .map(|k| &old.fills[old.fill_order[k] as usize]);
        for source in &mut fill.sources {
            let (id, ramp, fresh) = match source {
                ModSource::Live {
                    id,
                    ramp: Some(ramp),
                    fresh,
                    ..
                } => (*id, ramp, *fresh),
                ModSource::Live { ramp: None, .. } => continue,
                ModSource::Leaving { id, ramp, fresh } => (*id, ramp, *fresh),
            };
            let length = ramp.length;
            let previous = old_fill.and_then(|old_fill| old_fill.sources.iter().find(|source| source.id() == id));
            *ramp = if fresh {
                let held = previous.map_or(0.0, |previous| contribution_of(previous, old_assignments, frames));
                Ramp::new(held, length)
            } else {
                match previous.and_then(ModSource::ramp) {
                    Some(previous) if !previous.is_done() => {
                        let mut carried = Ramp::finished(length);
                        carried.continue_from(&previous);
                        carried
                    }
                    _ => Ramp::finished(length),
                }
            };
        }
    }
}

/// What an old source added to its parameter on the last frame of a block
/// of `frames` frames: 0 when it added nothing or that block never ran.
fn contribution_of(source: &ModSource, assignments: &[crate::compile::AssignmentRuntime], frames: usize) -> f32 {
    match source {
        ModSource::Leaving { ramp, .. } => {
            if frames == 0 {
                ramp.held()
            } else {
                ramp.value(0.0)
            }
        }
        ModSource::Live {
            state, buffer, ramp, ..
        } => {
            let Some(last) = frames.checked_sub(1) else {
                return ramp.map_or(0.0, |ramp| if ramp.is_done() { 0.0 } else { ramp.held() });
            };
            let Some(assignment) = assignments.get(*state as usize) else {
                return 0.0;
            };
            if !assignment.enabled || assignment.depth == 0.0 || assignment.silent_source {
                return 0.0;
            }
            // SAFETY: `buffer` is an output or delay tap of `max_block`
            // floats owned by the old schedule, which is alive here, and
            // its last block filled its first `frames <= max_block` floats.
            let sample = unsafe { *buffer.add(last) };
            let sample = match assignment.polarity {
                Some(polarity) => polarity.remap(assignment.source_polarity, sample),
                None => sample,
            };
            let live = assignment.depth * sample;
            ramp.map_or(live, |ramp| ramp.value(live))
        }
    }
}

#[cfg(test)]
mod tests;
