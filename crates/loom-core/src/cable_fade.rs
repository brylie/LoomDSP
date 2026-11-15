//! Click-free cable changes (live patching spec, rule 9; FR-GRAPH-05).
//!
//! When a schedule swap changes what feeds an input, the input holds the
//! last value it read and moves in a straight line to the new signal over
//! 5 ms, sample by sample. A modulation assignment that is added, removed,
//! or moved fades its contribution the same way. One turned on or off, or
//! given a new depth or polarity, blends its old setting into the new one
//! over 5 ms while its source plays (`ContributionRamp`).
//!
//! The compiler decides which inputs fade (`compile/fades.rs`) and gives
//! each one a fixed slot and buffer on the control thread. At the swap the
//! engine fills in the held values from the old schedule
//! (`take_state_from`), and each block it runs the ramps just before
//! their readers. Nothing here allocates, locks, or waits.
//!
//! Background: `docs/learn/hot-patching.md`.

use crate::compile::{ModSource, Schedule, fallback_fill};
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

/// The input of a Select that picks the member, read as a whole number.
pub const SELECT_MEMBER_INPUT: &str = "member";
/// The parameter of a Member module that holds the member it picks.
pub const MEMBER_PARAM: &str = "member";
/// The parameter of a Sum that scales the sum.
pub const SUM_GAIN_PARAM: &str = "gain";
/// The parameter of a Select that counts its members.
pub const SELECT_MEMBERS_PARAM: &str = "members";
/// The display value a Swap fade sets to 1 once it plays its new part alone.
pub const SWAP_FADE_DONE: &str = "done";

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

/// A straight line from one assignment setting to another, one step per
/// sample (live patching spec, rule 9, and How rule 9 handles assignment
/// changes).
///
/// A setting is a straight-line map of the source sample `x`, `g·x + h`
/// ([`AssignmentRuntime::map`](crate::compile::AssignmentRuntime::map)).
/// The ramp blends from the map `(g0, h0)` into what the current setting
/// gives. On frame `p` below `N`, with `t = p/N`, it gives
/// `old · (1 − t) + new · t`, where `old = g0·x + h0`, and from frame `N`
/// on `new` exactly. A ramp from a value has `g0 = 0`: it never reads the
/// source for `old`, so it gives the same values as [`Ramp`], and a NaN
/// source never reaches `h0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContributionRamp {
    g0: f32,
    h0: f32,
    position: u32,
    length: u32,
}

impl ContributionRamp {
    /// A ramp that is already over: it gives the current setting.
    pub(crate) fn finished(length: u32) -> Self {
        let length = length.max(1);
        Self {
            g0: 0.0,
            h0: 0.0,
            position: length,
            length,
        }
    }

    /// A ramp from a held value, as a swap's fresh ramp starts. A value
    /// that is NaN or infinite gives a finished ramp, so the contribution
    /// switches, as a held input value does (rule 9, The ramp).
    pub(crate) fn from_value(held: f32, length: u32) -> Self {
        if held.is_finite() {
            Self::from_map((0.0, held), length)
        } else {
            Self::finished(length)
        }
    }

    /// A ramp from the setting `(g0, h0)`, at its start.
    pub(crate) fn from_map((g0, h0): (f32, f32), length: u32) -> Self {
        Self {
            g0,
            h0,
            position: 0,
            length: length.max(1),
        }
    }

    /// A ramp that goes on where a value ramp `ramp` is.
    pub(crate) fn from_ramp(ramp: &Ramp, length: u32) -> Self {
        let mut carried = Self::from_map((0.0, ramp.held()), length);
        carried.position = ramp.position.min(carried.length);
        carried
    }

    /// Its length in frames.
    pub(crate) fn length(&self) -> u32 {
        self.length
    }

    /// True once the ramp gives the current setting unchanged.
    pub(crate) fn is_done(&self) -> bool {
        self.position >= self.length
    }

    /// Ends the ramp at once, as a panic does.
    pub(crate) fn finish(&mut self) {
        self.position = self.length;
    }

    /// True when the ramp reads the source sample for its old setting.
    #[inline]
    pub(crate) fn reads_source(&self) -> bool {
        !self.is_done() && self.g0 != 0.0
    }

    /// The map the ramp plays at its current position, heading for the
    /// setting `to`: the blend of the two, or `to` once it has ended.
    pub(crate) fn map_toward(&self, to: (f32, f32)) -> (f32, f32) {
        if self.is_done() {
            to
        } else {
            let t = self.position as f32 * (1.0 / self.length as f32);
            (self.g0 * (1.0 - t) + to.0 * t, self.h0 * (1.0 - t) + to.1 * t)
        }
    }

    /// Starts again from where it is, heading for the setting `played`
    /// was heading for: a change of settings (rule 9, A change while a
    /// blend runs). A blend of two finite maps is finite.
    pub(crate) fn restart_toward(&mut self, played: (f32, f32)) {
        (self.g0, self.h0) = self.map_toward(played);
        self.position = 0;
    }

    /// The contribution for source sample `x`, where `new` is what the
    /// current setting makes of it, without moving on.
    ///
    /// `hold_finite` holds only infinities: `f32::clamp` passes NaN
    /// through. NaN is kept out in other ways. While `g0` is 0 the source
    /// is not read, and a held value that is not finite makes a finished
    /// ramp ([`ContributionRamp::from_value`]). A NaN source read through
    /// a setting with `g0` not 0 gives NaN, as the setting alone would, and
    /// `ParamSpec::denormalize` turns a NaN position into the default.
    #[inline]
    pub(crate) fn value(&self, x: f32, new: f32) -> f32 {
        if self.position >= self.length {
            return new;
        }
        let old = if self.g0 == 0.0 {
            self.h0
        } else {
            hold_finite(self.g0 * x + self.h0)
        };
        if self.position == 0 {
            old
        } else {
            let t = self.position as f32 * (1.0 / self.length as f32);
            // The two-weight form, as `Ramp::value` uses (ADR-0037).
            hold_finite(old * (1.0 - t) + new * t)
        }
    }

    /// The contribution for `x` and `new`, then one step on.
    #[inline]
    pub(crate) fn next(&mut self, x: f32, new: f32) -> f32 {
        let value = self.value(x, new);
        self.position = self.position.saturating_add(1).min(self.length);
        value
    }

    /// The same ramp, continued from `other`'s map and position.
    pub(crate) fn continue_from(&mut self, other: &ContributionRamp) {
        self.g0 = other.g0;
        self.h0 = other.h0;
        self.position = other.position.min(self.length);
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
    /// For a fresh fade with nothing to read, the channel of the same
    /// input whose start it takes: the left side for a right side, copy 0
    /// for a new copy of a module. `None` starts from 0, as a new voice
    /// does where voices are summed (the audio output, a bundle input).
    fallback: Option<u16>,
    /// Set at the swap while the held value is still to be found.
    pending: bool,
    target: *const f32,
    buffer: *mut f32,
}

impl InputFade {
    /// A fade from `target` into `buffer`. The held value is set at the
    /// swap.
    pub(crate) fn new(
        key: u64,
        (fresh, fallback): (bool, Option<u16>),
        length: u32,
        target: *const f32,
        buffer: *mut f32,
    ) -> Self {
        debug_assert!(!std::ptr::eq(target, buffer), "a fade reads another buffer");
        debug_assert!(
            fallback.is_none_or(|channel| usize::from(channel) < key_channel(key)),
            "a fade takes its start from a lower channel"
        );
        Self {
            key,
            ramp: Ramp::finished(length),
            fresh,
            fallback,
            pending: false,
            target,
            buffer,
        }
    }

    /// The buffer the reader reads.
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

/// Where a fresh fade on channel `channel` takes its start when it has
/// nothing of its own to read: a right side from its own voice's left side,
/// a new copy of a module (`copies`) from copy 0, and a new voice that is
/// summed with the others from nothing.
pub(crate) fn fallback(channel: usize, copies: bool) -> Option<u16> {
    if channel % 2 == 1 {
        u16::try_from(channel - 1).ok()
    } else if channel > 0 && copies {
        Some(0)
    } else {
        None
    }
}

/// The last of the first `frames` samples at `ptr`, or `None` when no frame
/// ran.
fn last_sample(ptr: *const f32, frames: usize) -> Option<f32> {
    let last = frames.checked_sub(1)?;
    // SAFETY: callers pass a buffer of `max_block` floats owned by the old
    // schedule, which is alive during the swap, and `frames` is the length
    // of that schedule's last block, so `last < frames <= max_block`.
    Some(unsafe { *ptr.add(last) })
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

/// Starts the setting ramps that the commands of one batch call for
/// (live patching spec, rule 9, Turned off or on). Runs on the audio
/// thread after the whole batch, before the next block, and only when a
/// command changed an assignment.
///
/// Each source with a ramp slot whose assignment changed, and whose
/// settings now map the source differently from the ones it was heading
/// for (`played`), starts a ramp from where it is. Maps are compared, not
/// settings, because two settings can map alike: off and depth 0, or a
/// polarity forced to the source's own. A batch that ends on a map that
/// plays starts nothing, so it is bit-identical to no command. Before the
/// engine's first block (`playing` false) nothing starts: nobody heard the
/// old setting. Then every changed runtime's `played` takes its settings.
/// One visit per source, no allocation.
pub(crate) fn start_setting_ramps(schedule: &mut Schedule, playing: bool) {
    if !schedule.assignments_changed {
        return;
    }
    let assignments = &schedule.assignments;
    if playing {
        for fill in &mut schedule.fills {
            for source in &mut fill.sources {
                let ModSource::Live {
                    state,
                    ramp: Some(ramp),
                    ..
                } = source
                else {
                    continue;
                };
                let Some(assignment) = assignments.get(*state as usize) else {
                    continue;
                };
                let played = assignment.map(assignment.played);
                if assignment.changed && assignment.map(assignment.settings()) != played {
                    ramp.restart_toward(played);
                }
            }
        }
    }
    // A separate loop, so a runtime with no slot on any source, onto a
    // parameter that switches, is cleared too.
    for assignment in &mut schedule.assignments {
        if assignment.changed {
            assignment.played = assignment.settings();
            assignment.changed = false;
        }
    }
    schedule.assignments_changed = false;
}

/// What an input read last in `old`, whose last block was `frames` frames.
/// Reads memory only when that block ran. When it did not, an input that
/// read a fixed default gives that value, and anything else gives `None`.
fn last_read(old: &Schedule, key: u64, frames: usize) -> Option<f32> {
    let i = old.reads.binary_search_by_key(&key, |read| read.key).ok()?;
    let read = &old.reads[i];
    // Every pointer in `reads` names a buffer of `max_block` floats in `old`.
    last_sample(read.ptr, frames).or(read.constant)
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
            fade.pending = held.is_none();
            // A pending fade is filled below, from another channel or 0.
            Ramp::new(held.unwrap_or(0.0), length)
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
    // A fresh fade with nothing to read takes the start of a lower channel
    // of the same input (its left side, or copy 0, rule 6), or starts from
    // 0. Keys sort by channel, so that channel is already settled here.
    for n in 0..new.cable_fade_order.len() {
        let i = new.cable_fade_order[n] as usize;
        let fade = &new.cable_fades[i];
        if !fade.pending {
            continue;
        }
        let length = fade.ramp.length;
        let from = fade.fallback.and_then(|channel| {
            let key = (fade.key & !0xFFFF) | u64::from(channel);
            let k = new
                .cable_fade_order
                .binary_search_by_key(&key, |&k| new.cable_fades[k as usize].key)
                .ok()?;
            Some(new.cable_fades[new.cable_fade_order[k] as usize].ramp)
        });
        let fade = &mut new.cable_fades[i];
        fade.pending = false;
        if let Some(from) = from {
            fade.ramp = Ramp::finished(length);
            fade.ramp.continue_from(&from);
        }
    }
    take_contributions_from(new, old, frames);
}

/// The source of assignment `id` on the fill with key `fill` in `old`.
/// Every lookup is a binary search and a checked index, so a stale index
/// cannot panic on the audio thread. A live source whose runtime `old`
/// does not have counts as no match.
fn old_source(old: &Schedule, fill: u64, id: crate::graph::AssignmentId) -> Option<&ModSource> {
    let k = old
        .fill_order
        .binary_search_by_key(&fill, |&k| old.fills.get(k as usize).map_or(u64::MAX, |fill| fill.key))
        .ok()?;
    let old_fill = old.fills.get(*old.fill_order.get(k)? as usize)?;
    // A fill's sources are sorted by assignment id.
    let s = old_fill.sources.binary_search_by_key(&id, ModSource::id).ok()?;
    let source = old_fill.sources.get(s)?;
    match source {
        ModSource::Live { state, .. } if old.assignments.get(*state as usize).is_none() => None,
        _ => Some(source),
    }
}

/// The ramp a live source that is not fresh takes from its old match
/// `previous`, when its settings now map the source as `map` (rule 9, A
/// schedule swap).
///
/// The same map as the old runtime's `played` carries the old ramp on.
/// Another map, as an undo brings back, starts a ramp from where the old
/// one was, heading for the setting it played. Maps are compared, not
/// settings, as in [`start_setting_ramps`]. A leaving source carries its
/// value ramp on.
fn carried_ramp(previous: &ModSource, old: &Schedule, map: Option<(f32, f32)>, length: u32) -> ContributionRamp {
    match previous {
        ModSource::Leaving { ramp, .. } if !ramp.is_done() => ContributionRamp::from_ramp(ramp, length),
        ModSource::Leaving { .. } => ContributionRamp::finished(length),
        ModSource::Live { state, ramp, .. } => {
            let Some(runtime) = old.assignments.get(*state as usize) else {
                return ContributionRamp::finished(length);
            };
            let played = runtime.map(runtime.played);
            if map == Some(played) {
                let mut carried = ContributionRamp::finished(length);
                if let Some(ramp) = ramp {
                    carried.continue_from(ramp);
                }
                carried
            } else {
                let from = ramp.as_ref().map_or(played, |ramp| ramp.map_toward(played));
                ContributionRamp::from_map(from, length)
            }
        }
    }
}

/// Fills in the contribution ramps and last values of `new`'s fills from
/// `old`, whose last block was `frames` frames (rule 9, A schedule swap).
///
/// - A fresh source ramps from a value: what the old source added last.
/// - A live source that is not fresh carries its ramp on, or blends from
///   the setting that played when its map differs ([`carried_ramp`]).
/// - A leaving source ramps from a value, or carries a leaving ramp on.
/// - A source with no old match on a right side or a new copy takes the
///   state of the same assignment on its fallback fill, in a second pass.
///
/// Copies values only. No allocation, binary searches only.
fn take_contributions_from(new: &mut Schedule, old: &Schedule, frames: usize) {
    let assignments = &new.assignments;
    for fill in &mut new.fills {
        let key = fill.key;
        for source in &mut fill.sources {
            let previous = old_source(old, key, source.id());
            let held = previous.map_or(0.0, |previous| contribution_of(previous, frames));
            match source {
                ModSource::Live {
                    state,
                    ramp,
                    last,
                    fresh,
                    ..
                } => {
                    // Copied for every match, so a fresh ramp in a second
                    // swap before any block starts from what played.
                    *last = held;
                    let Some(ramp) = ramp else {
                        continue;
                    };
                    let length = ramp.length();
                    *ramp = match previous {
                        _ if *fresh => ContributionRamp::from_value(held, length),
                        Some(previous) => carried_ramp(
                            previous,
                            old,
                            assignments
                                .get(*state as usize)
                                .map(|runtime| runtime.map(runtime.settings())),
                            length,
                        ),
                        None => ContributionRamp::finished(length),
                    };
                }
                ModSource::Leaving { ramp, fresh, .. } => {
                    let length = ramp.length;
                    // A map ramp is never carried into a leaving source:
                    // one whose old source was live starts from its value.
                    *ramp = match previous {
                        Some(ModSource::Leaving { ramp: previous, .. }) if !*fresh => {
                            let mut carried = Ramp::finished(length);
                            if !previous.is_done() {
                                carried.continue_from(previous);
                            }
                            carried
                        }
                        Some(_) => Ramp::new(held, length),
                        None if *fresh => Ramp::new(0.0, length),
                        None => Ramp::finished(length),
                    };
                }
            }
        }
    }
    take_fallback_contributions(new, old);
}

/// The second pass of [`take_contributions_from`]: a live source with no
/// old match on a right side or a new copy takes `last` and the ramp of
/// the same assignment on its fallback fill (the left side, or copy 0),
/// so both blend together. Runs in key order, so a fallback fill, whose
/// key is lower, is already settled.
fn take_fallback_contributions(new: &mut Schedule, old: &Schedule) {
    for n in 0..new.fill_order.len() {
        let i = new.fill_order[n] as usize;
        let Some(fallback) = fallback_fill(new.fills[i].key) else {
            continue;
        };
        let Ok(f) = new
            .fill_order
            .binary_search_by_key(&fallback, |&k| new.fills[k as usize].key)
        else {
            continue;
        };
        let f = new.fill_order[f] as usize;
        for s in 0..new.fills[i].sources.len() {
            let source = &new.fills[i].sources[s];
            let id = source.id();
            if !matches!(source, ModSource::Live { .. }) || old_source(old, new.fills[i].key, id).is_some() {
                continue;
            }
            let from = &new.fills[f].sources;
            let Some(&ModSource::Live {
                ramp: from_ramp,
                last: from_last,
                ..
            }) = from
                .binary_search_by_key(&id, ModSource::id)
                .ok()
                .and_then(|k| from.get(k))
            else {
                continue;
            };
            if let ModSource::Live { ramp, last, .. } = &mut new.fills[i].sources[s] {
                *last = from_last;
                if let (Some(ramp), Some(from_ramp)) = (ramp, from_ramp) {
                    ramp.continue_from(&from_ramp);
                }
            }
        }
    }
}

/// What an old source added to its parameter on the last frame it ran.
/// A live source reads its `last`, whether or not the old schedule ran,
/// so a command in the same batch cannot change it. A leaving one reads
/// its ramp.
fn contribution_of(source: &ModSource, frames: usize) -> f32 {
    match source {
        ModSource::Leaving { ramp, .. } => {
            if frames == 0 {
                ramp.held()
            } else {
                ramp.value(0.0)
            }
        }
        ModSource::Live { last, .. } => *last,
    }
}

#[cfg(test)]
mod tests;
