//! The compiled schedule the engine runs, and the operations and buffers it holds.

use super::*;

pub(crate) struct ModuleOp {
    pub slot: u32,
    pub inputs: (u32, u32),
    pub outputs: (u32, u32),
    pub connected: u64,
    /// Range into [`Schedule::fills`].
    pub fills: (u32, u32),
    /// Range into [`Schedule::param_ptrs`], one entry per modulated parameter.
    pub params: (u32, u32),
    /// Range into [`Schedule::downmixes`], filled before the module runs.
    pub downmixes: (u32, u32),
    /// Range into [`Schedule::right_ptrs`], for `Stereo` ports.
    pub right: (u32, u32),
    /// 0 for a module that runs as one, or its left copy; 1 for its right.
    pub channel: u8,
}

/// How a downmix makes one channel from two (stereo spec, rule 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DownmixKind {
    /// `(left + right) / 2`, for audio, modulation, and pitch.
    Mean,
    /// `max(left, right)`, for gates: high when either side is high.
    Max,
    /// The mean the shorter way round the circle, for phases.
    Phase,
}

impl DownmixKind {
    /// The method the input's signal type picks.
    pub fn of(signal: SignalType) -> Self {
        match signal {
            SignalType::Gate => Self::Max,
            SignalType::Phase => Self::Phase,
            SignalType::Audio | SignalType::Modulation | SignalType::Pitch => Self::Mean,
        }
    }

    /// One sample of the downmix.
    #[inline]
    pub fn apply(self, left: f32, right: f32) -> f32 {
        match self {
            Self::Mean => (left + right) * 0.5,
            Self::Max => left.max(right),
            Self::Phase => {
                // The difference the short way round, from -0.5 (included)
                // up to 0.5 (not included). Exactly half a cycle apart, it
                // is -0.5, so the tie always goes the same way.
                let difference = right - left;
                let difference = difference - (difference + 0.5).floor();
                let mean = left + difference * 0.5;
                mean - mean.floor()
            }
        }
    }
}

/// Fills `output` from two channels just before a module runs.
pub(crate) struct DownmixOp {
    pub kind: DownmixKind,
    pub left: *const f32,
    pub right: *const f32,
    pub output: *mut f32,
}

/// One source of a modulated parameter. `state` indexes [`Schedule::assignments`].
pub(crate) struct ModSource {
    pub state: u32,
    pub buffer: *const f32,
}

/// A parameter the engine fills before its module runs.
pub(crate) struct ParamFill {
    /// `(module id << 32) | param index`. Matches smoother state across swaps.
    pub key: u64,
    pub slot: u32,
    pub param: u16,
    /// The parameter limited to its selected range, so modulation maps and
    /// clamps inside it (control-ranges.md, rules 4, 5, and 8).
    pub spec: ParamSpec,
    pub base: f32,
    pub smooth: Option<Smoothed>,
    pub buffer: *mut f32,
    pub sources: Vec<ModSource>,
    /// Cell in the live value table, or [`crate::live::NO_CELL`]. Set by
    /// the controller after compiling.
    pub live: u32,
}

impl ParamFill {
    /// True for the fill of a module's right copy. Its live value is not
    /// shown: the knob shows copy 0 (stereo spec, rule 16).
    pub fn is_right_copy(&self) -> bool {
        self.key & RIGHT_FILL != 0
    }
}

/// Depth, polarity, and enabled flag. The audio thread updates these without
/// recompiling. Sorted by [`AssignmentRuntime::id`].
pub(crate) struct AssignmentRuntime {
    pub id: AssignmentId,
    pub depth: f32,
    pub enabled: bool,
    pub polarity: Option<Polarity>,
    pub source_polarity: Polarity,
    /// A missing source must remain inert even under a polarity override.
    pub silent_source: bool,
}

pub(crate) struct Tap {
    pub key: u64,
    /// [`output_key`] of the output this delay reads, or [`NO_SOURCE`].
    /// A new delay starts with that output's last value (live patching
    /// spec, rule 5).
    pub source: u64,
    pub input: *const f32,
    pub output: *mut f32,
    pub state: f32,
}

pub(crate) struct OutputOp {
    pub left: *const f32,
    pub right: *const f32,
}

/// A probed output buffer. Read after each block for telemetry.
pub(crate) struct ProbeOp {
    pub id: ProbeId,
    pub buffer: *const f32,
    pub right: Option<*const f32>,
    pub kind: ProbeKind,
}

/// Buffers for transport signals that at least one unconnected input reads.
/// `None` when nothing reads that signal, so it costs nothing.
#[derive(Default)]
pub(crate) struct TransportBuffers {
    pub bar_phase: Option<*mut f32>,
    pub beat_phase: Option<*mut f32>,
    pub playing: Option<*mut f32>,
    /// Engine slot of the Transport module, if the patch has one.
    pub module: Option<u32>,
    /// True when the Transport's `run` or `reset` input has a cable: the
    /// engine then splits blocks on the transport grid (rule 11).
    pub grid: bool,
    /// True when the Transport's `run` input has a cable, so a fresh
    /// Transport waits for the gate instead of sending its Run parameter.
    pub run_cable: bool,
}

pub(crate) enum Step {
    /// Run a module over the whole block.
    Module(u32),
    /// Run a unit delay over the whole block.
    Tap(u32),
    /// Run a feedback loop one sample at a time.
    Loop { ops: (u32, u32), taps: (u32, u32) },
}

/// A compiled, ready-to-run graph. Built on the control thread, then moved to
/// the audio thread whole.
pub(crate) struct Schedule {
    // Owned buffer memory. The raw pointers below point into these boxes.
    // Moving the `Schedule` moves the boxes, not the heap data, so the
    // pointers stay valid.
    pub(super) _buffers: Vec<Box<[f32]>>,
    pub(super) _zero: Box<[f32]>,
    pub input_ptrs: Vec<*const f32>,
    pub output_ptrs: Vec<*mut f32>,
    pub ops: Vec<ModuleOp>,
    /// Sorted by key, so tap state can be carried across schedule swaps.
    pub taps: Vec<Tap>,
    /// Every output port's buffer, sorted by [`output_key`]. The next
    /// schedule reads it at the swap to start its new delays.
    pub sources: Vec<(u64, *const f32)>,
    pub loop_ops: Vec<u32>,
    pub loop_taps: Vec<u32>,
    pub steps: Vec<Step>,
    pub outputs: Vec<OutputOp>,
    pub probes: Vec<ProbeOp>,
    pub transport: TransportBuffers,
    pub fills: Vec<ParamFill>,
    /// Indices into `fills`, sorted by fill key. Built on the control thread
    /// so [`Schedule::take_smooth_state_from`] can merge two sorted lists.
    pub fill_order: Vec<u32>,
    pub assignments: Vec<AssignmentRuntime>,
    pub param_ptrs: Vec<(u16, *const f32)>,
    /// Downmixes, run by the module operation that owns them, or before the
    /// outputs are summed (`output_downmixes`).
    pub downmixes: Vec<DownmixOp>,
    pub output_downmixes: (u32, u32),
    pub right_ptrs: Vec<RightPtr>,
    /// `(left slot, right slot)` of every module that runs as two copies,
    /// sorted by left slot. A parameter change or reset on the left slot
    /// also goes to the right one.
    pub copies: Vec<(u32, u32)>,
    /// MIDI mappings, sorted by control.
    pub midi_maps: Vec<MidiMapOp>,
    /// Live value cells the previous schedule used and this one does not.
    /// The engine releases them when it swaps this schedule in.
    pub live_release: Vec<u32>,
}

// SAFETY: the raw pointers only point into buffers owned by this schedule.
// The schedule is only ever used by one thread at a time (built on the control
// thread, then moved to the audio thread, then moved back for dropping).
unsafe impl Send for Schedule {}

impl Schedule {
    pub fn empty() -> Self {
        Self {
            _buffers: Vec::new(),
            _zero: Box::new([]),
            input_ptrs: Vec::new(),
            output_ptrs: Vec::new(),
            ops: Vec::new(),
            taps: Vec::new(),
            sources: Vec::new(),
            loop_ops: Vec::new(),
            loop_taps: Vec::new(),
            steps: Vec::new(),
            outputs: Vec::new(),
            probes: Vec::new(),
            transport: TransportBuffers::default(),
            fills: Vec::new(),
            fill_order: Vec::new(),
            assignments: Vec::new(),
            param_ptrs: Vec::new(),
            downmixes: Vec::new(),
            output_downmixes: (0, 0),
            right_ptrs: Vec::new(),
            copies: Vec::new(),
            midi_maps: Vec::new(),
            live_release: Vec::new(),
        }
    }

    /// Copies unit delay state from the previous schedule, matched by key.
    /// A delay with no state to carry starts with the last value its source
    /// output wrote in the old schedule's last block of `frames` frames, or
    /// 0 if that output did not run (live patching spec, rule 5). Then the
    /// delay holds what it would hold had it always been there.
    /// Runs on the audio thread: no allocation, linear time plus a binary
    /// search per new delay.
    pub fn take_tap_state_from(&mut self, old: &Schedule, frames: usize) {
        let mut j = 0;
        for tap in &mut self.taps {
            while j < old.taps.len() && old.taps[j].key < tap.key {
                j += 1;
            }
            if j < old.taps.len() && old.taps[j].key == tap.key {
                tap.state = old.taps[j].state;
            } else if tap.key & RIGHT_TAP != 0
                && let Ok(i) = old
                    .taps
                    .binary_search_by_key(&(tap.key & !RIGHT_TAP), |old_tap| old_tap.key)
            {
                // A new right channel inherits the running mono delay.
                tap.state = old.taps[i].state;
            } else if let Some(last) = frames.checked_sub(1)
                && tap.source != NO_SOURCE
                && let Some(i) = old
                    .sources
                    .binary_search_by_key(&tap.source, |&(key, _)| key)
                    .ok()
                    .or_else(|| {
                        (tap.source & RIGHT_TAP != 0)
                            .then(|| {
                                old.sources
                                    .binary_search_by_key(&(tap.source & !RIGHT_TAP), |&(key, _)| key)
                                    .ok()
                            })
                            .flatten()
                    })
            {
                // SAFETY: every buffer in `sources` is `max_block` floats
                // long and owned by `old`, which is alive here. The last
                // block filled its first `frames <= max_block` floats.
                tap.state = unsafe { *old.sources[i].1.add(last) };
            }
        }
    }

    /// Copies in-flight base smoothing onto the new schedule, matched by key.
    /// A fill that is new, because its parameter just got its first
    /// assignment, starts from `glide(slot, param)` instead: the value the
    /// module itself was gliding through, so the parameter does not step.
    /// Fills sit in execution order, which is not key order, so both
    /// schedules keep a `fill_order` index sorted by key. Walking the two
    /// indices together matches every key in one pass, like
    /// [`Schedule::take_tap_state_from`]. The order matters: a merge only
    /// works if both lists are sorted by the same key.
    /// Runs on the audio thread: no allocation, linear time.
    pub fn take_smooth_state_from(&mut self, old: &Schedule, glide: impl Fn(usize, usize) -> Option<f32>) {
        let mut j = 0;
        for &i in &self.fill_order {
            let fill = &mut self.fills[i as usize];
            while j < old.fill_order.len() && old.fills[old.fill_order[j] as usize].key < fill.key {
                j += 1;
            }
            let Some(smooth) = fill.smooth.as_mut() else {
                continue;
            };
            let previous = old
                .fill_order
                .get(j)
                .map(|&k| &old.fills[k as usize])
                .filter(|previous| previous.key == fill.key)
                .map(|previous| previous.smooth);
            match previous {
                // The range changed: the glide goes on from the value it had
                // reached, at that value's place in the new range, so the
                // sound does not jump and no value is passed that was never
                // set (control-ranges.md).
                Some(Some(mut previous)) if previous_spec(old, j) != Some((fill.spec.min, fill.spec.max)) => {
                    let old_spec = old.fills[old.fill_order[j] as usize].spec;
                    let value = old_spec.denormalize(previous.current());
                    previous.reset(fill.spec.normalize(value));
                    previous.set_target(fill.spec.normalize(fill.base));
                    *smooth = previous;
                }
                Some(Some(previous)) => *smooth = previous,
                Some(None) => {}
                None => {
                    if let Some(value) = glide(fill.slot as usize, usize::from(fill.param)) {
                        smooth.reset(fill.spec.normalize(value));
                        smooth.set_target(fill.spec.normalize(fill.base));
                    }
                }
            }
        }
    }
}
