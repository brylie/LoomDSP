//! Test harness for LoomDSP modules (FR-SDK-02).
//!
//! Renders one module with given inputs and parameters, checks that block
//! size does not change its output, and compares it with a stored reference
//! render. Spec: `docs/design/module-test-harness.md`.
//!
//! ```no_run
//! use loom_test::{Harness, Signal, reference};
//!
//! # static SINE: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
//! #     id: "doc.sine", version: 1, name: "Sine", category: "Doc", family: loom_core::Family::Sources, description: "",
//! #     inputs: &[], outputs: &[], params: &[], role: loom_core::ModuleRole::UnitDelay,
//! #     panel: loom_core::Panel::EMPTY, displays: &[], roles: &[],
//! # };
//! Harness::new(&SINE)
//!     .seconds(0.05)
//!     .input("phase", Signal::phase(440.0))
//!     .check(reference!("sine"));
//! ```
//!
//! References are 32-bit float WAV files, so a reviewer can listen to them.
//! Create or update them with `LOOM_UPDATE_REFERENCES=1 cargo test`.
//!
//! This crate reads and writes files, so it is for tests on the desktop
//! only. Every crate lists it under `[dev-dependencies]`, so it never ships.
//! The measurements of recorded audio, which `loom analyze` ships, are in
//! `loom-analysis`, and the `test.` modules are in
//! `loom_modules::test_modules`.

#![warn(missing_docs)]

mod reference;
mod signal;

pub use reference::{UPDATE_VAR, check_channels};
pub use signal::Signal;

use loom_core::{
    MidiEvent, Module, ModuleConfig, ModuleDescriptor, ModuleRole, PortDefault, SignalType, TransportInfo,
    TransportSettings, testing,
};
use std::path::Path;

/// Block sizes that [`Harness::check_block_sizes`] compares.
pub const BLOCK_SIZES: [usize; 3] = [1, 77, MAX_BLOCK];

/// `max_block` for every module the harness creates, and the block size of
/// reference renders.
pub const MAX_BLOCK: usize = 512;

/// Default largest difference from a reference, per sample.
pub const DEFAULT_TOLERANCE: f32 = 1e-5;

/// First frame of the non-finite input in
/// [`Harness::check_non_finite_recovery`]. The module runs normally before
/// it, so it has state to lose. The bad frames cover [`BAD_EDGE`].
pub const BAD_START: usize = 1_068;

/// How many frames the non-finite input lasts in
/// [`Harness::check_non_finite_recovery`].
pub const BAD_FRAMES: usize = 8;

/// The rising edge of [`Signal::non_finite_busy`] inside the bad frames. A
/// module that stores an input on a trigger on another input takes the bad
/// value here.
pub const BAD_EDGE: usize = 1_072;

/// The next rising edge of [`Signal::non_finite_busy`]. It overwrites bad state
/// that [`BAD_EDGE`] captured, so the recovery time must end before it, or the
/// check would pass a module that kept the bad value until then.
pub const BAD_NEXT_EDGE: usize = 2_032;

/// Time a module may take to give finite output again after the
/// non-finite input ends, in [`Harness::check_non_finite_recovery`], in
/// seconds. The check turns it into frames at the harness's sample rate, so
/// the rule is the same 10 ms at any rate (480 frames at 48 kHz).
pub const RECOVERY_SECONDS: f64 = 0.010;

/// Frame where [`Harness::check_extreme_recovery`] starts to hold the extreme
/// value. It holds across the first rising edge of [`Signal::extreme_busy`], so
/// state captured on a trigger sees the value.
pub const EXTREME_START: usize = 2_300;

/// How long [`Harness::check_extreme_recovery`] holds the extreme value, in
/// frames.
pub const EXTREME_FRAMES: usize = 960;

/// Frame where one pass of [`Harness::check_extreme_recovery`] flips the sign,
/// after the first rising edge. Subtracting a new huge value from a stored one
/// of the other sign overflows, which holding one sign never shows.
pub const EXTREME_FLIP: usize = 2_900;

/// Frames from the end of the hold until output must match a clean render, in
/// [`Harness::check_extreme_recovery`]. A huge value takes time to leave a slow
/// filter or envelope.
pub const EXTREME_SETTLE: usize = 12_000;

/// The second rising edge of [`Signal::extreme_busy`]. That edge overwrites bad
/// state captured on the first one, so the comparison must end here, or it
/// would pass a module that kept the bad value until then.
pub const EXTREME_NEXT_EDGE: usize = 26_788;

/// The shortest render for [`Harness::check_extreme_recovery`]: two periods of
/// [`Signal::extreme_busy`].
pub const EXTREME_RENDER: usize = 48_000;

/// Default sample rate, in Hz.
pub const DEFAULT_SAMPLE_RATE: f32 = 48_000.0;

/// Path of the reference file `name` in the calling crate:
/// `tests/reference/<name>.wav`.
#[macro_export]
macro_rules! reference {
    ($name:literal) => {
        ::std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/reference/", $name, ".wav"))
    };
}

/// Which inputs take the huge value in the extreme check.
#[derive(Clone, Copy)]
enum ExtremeTarget {
    /// Each input in turn, the others staying normal.
    Each,
    /// One input, by index.
    One(usize),
    /// Every input together.
    AllAtOnce,
}

/// What the extreme check requires of the outputs. Each level includes the ones
/// below it.
#[derive(Clone, Copy, PartialEq)]
enum ExtremeLevel {
    /// Matches a clean render after the settling time.
    Recovers,
    /// Returns to a normal size after the next edge.
    Bounded,
    /// Stays finite for the whole render.
    Finite,
}

enum Length {
    Frames(usize),
    Seconds(f64),
}

/// A test render of one module. Build it with the setter methods, then call
/// [`Harness::check`] or one of the other run methods.
pub struct Harness {
    descriptor: &'static ModuleDescriptor,
    sample_rate: f32,
    length: Length,
    inputs: Vec<Option<Signal>>,
    bundle_inputs: std::collections::BTreeMap<usize, (loom_core::Width, Vec<Signal>)>,
    params: Vec<(usize, f32)>,
    midi: Vec<MidiEvent>,
    transport: TransportSettings,
    /// Parameter changes during the render: `(seconds, index, value)`.
    param_changes: Vec<(f64, usize, f32)>,
    /// Transport changes during the render: `(seconds, settings)`.
    transport_changes: Vec<(f64, TransportSettings)>,
    tolerance: f32,
    allow_undocumented: bool,
    /// First bad frame for the non-finite recovery check.
    non_finite_start: usize,
}

/// The transport from one frame of a render until the next change.
#[derive(Clone, Copy)]
struct TransportSpan {
    /// The frame where this transport takes effect.
    start: usize,
    /// The settings in force, so the next change can tell what it changed.
    settings: TransportSettings,
    /// The transport at `start`.
    info: TransportInfo,
}

/// The transport in force at `frame`, for a call that starts there.
fn transport_at_frame(spans: &[TransportSpan], frame: usize) -> TransportInfo {
    let span = spans[spans.partition_point(|span| span.start <= frame) - 1];
    span.info.later(frame - span.start)
}

impl Harness {
    /// A harness for a DSP module. Panics for a unit delay or an audio
    /// output, which the engine runs itself.
    #[track_caller]
    pub fn new(descriptor: &'static ModuleDescriptor) -> Self {
        assert!(
            matches!(descriptor.role, ModuleRole::Dsp(_)),
            "{}: the harness tests DSP modules; the engine runs unit delays and audio outputs itself",
            descriptor.id
        );
        Self {
            descriptor,
            sample_rate: DEFAULT_SAMPLE_RATE,
            length: Length::Seconds(0.1),
            inputs: descriptor.inputs.iter().map(|_| None).collect(),
            bundle_inputs: std::collections::BTreeMap::new(),
            params: Vec::new(),
            midi: Vec::new(),
            transport: TransportSettings {
                running: false,
                ..TransportSettings::default()
            },
            param_changes: Vec::new(),
            transport_changes: Vec::new(),
            tolerance: DEFAULT_TOLERANCE,
            allow_undocumented: false,
            non_finite_start: BAD_START,
        }
    }

    /// Sample rate of the render and of its reference file. Default 48 000.
    pub fn sample_rate(mut self, hz: f32) -> Self {
        self.sample_rate = hz;
        self
    }

    /// Length of the render in seconds, rounded to whole frames. Default 0.1.
    pub fn seconds(mut self, seconds: f64) -> Self {
        self.length = Length::Seconds(seconds);
        self
    }

    /// Length of the render in frames.
    pub fn frames(mut self, frames: usize) -> Self {
        self.length = Length::Frames(frames);
        self
    }

    /// First frame of bad input for [`Harness::check_non_finite_recovery`].
    /// Defaults to [`BAD_START`]. A custom start can cover a transport wrap on
    /// port defaults. [`Signal::non_finite_busy`] aligns only with the default.
    pub fn non_finite_start(mut self, frame: usize) -> Self {
        self.non_finite_start = frame;
        self
    }

    /// Connects input `name` to `signal`. Inputs left out read their port
    /// default, as in the engine.
    #[track_caller]
    pub fn input(mut self, name: &str, signal: Signal) -> Self {
        let Some(index) = self.descriptor.inputs.iter().position(|port| port.name == name) else {
            panic!(
                "{}: no input `{name}`; it has {:?}",
                self.descriptor.id,
                self.descriptor.inputs.iter().map(|port| port.name).collect::<Vec<_>>()
            );
        };
        self.bundle_inputs.remove(&index);
        self.inputs[index] = Some(signal);
        self
    }

    /// Connects one mono signal per voice to a Voices input.
    pub fn bundle_input(self, name: &str, voices: Vec<Signal>) -> Self {
        let width = loom_core::Width {
            voices: voices.len(),
            sides: 1,
        };
        self.bundle_signals(name, width, voices)
    }

    /// Connects a left/right signal pair per voice to a Voices input.
    pub fn stereo_bundle_input(self, name: &str, voices: Vec<[Signal; 2]>) -> Self {
        let width = loom_core::Width {
            voices: voices.len(),
            sides: 2,
        };
        self.bundle_signals(name, width, voices.into_iter().flatten().collect())
    }

    /// Validates and stores a bundle's signals in voice/side order.
    fn bundle_signals(mut self, name: &str, width: loom_core::Width, signals: Vec<Signal>) -> Self {
        assert!((1..=16).contains(&width.voices));
        let port = self.descriptor.input_index(name).expect("bundle input exists");
        assert_eq!(self.descriptor.inputs[port].channels, loom_core::Channels::Voices);
        self.inputs[port] = None;
        self.bundle_inputs.insert(port, (width, signals));
        self
    }

    /// Layout of rendered input buffers, including explicit bundle signals.
    fn input_layout(&self) -> testing::BundleLayout {
        testing::BundleLayout::new(
            (0..self.descriptor.inputs.len())
                .map(|port| {
                    self.bundle_inputs
                        .get(&port)
                        .map_or(loom_core::Width::MONO, |(width, _)| *width)
                })
                .collect(),
        )
    }

    /// Output layout at a frame, or maximum capacity across a scripted render.
    fn output_layout(&self, frame: Option<usize>) -> testing::BundleLayout {
        let mut voices = 1;
        if let Some(index) = self.descriptor.param_index("voices") {
            let mut count = self.descriptor.params[index].default;
            for &(param, value) in &self.params {
                if param == index {
                    count = value;
                }
            }
            voices = count.round().clamp(1.0, 16.0) as usize;
            for (at, param, value) in self.scheduled_params() {
                if param == index && frame.is_none_or(|frame| at <= frame) {
                    let next = value.round().clamp(1.0, 16.0) as usize;
                    voices = if frame.is_none() { voices.max(next) } else { next };
                }
            }
        }
        let bundle_aware = self
            .descriptor
            .inputs
            .iter()
            .chain(self.descriptor.outputs)
            .any(|port| port.channels == loom_core::Channels::Voices);
        let sides = self
            .bundle_inputs
            .values()
            .map(|(width, _)| width.sides)
            .max()
            .unwrap_or(1);
        testing::BundleLayout::new(
            self.descriptor
                .outputs
                .iter()
                .map(|port| {
                    if port.channels == loom_core::Channels::Voices {
                        loom_core::Width { voices, sides: 1 }
                    } else if bundle_aware {
                        let sides = if port.channels == loom_core::Channels::Stereo
                            || (port.channels == loom_core::Channels::PerChannel && sides == 2)
                        {
                            2
                        } else {
                            1
                        };
                        loom_core::Width { voices: 1, sides }
                    } else {
                        loom_core::Width::MONO
                    }
                })
                .collect(),
        )
    }

    /// Sets parameter `name`, clamped to its range as
    /// `Controller::set_param` clamps it.
    #[track_caller]
    pub fn param(mut self, name: &str, value: f32) -> Self {
        let index = self.param_named(name);
        self.params.push((index, self.descriptor.params[index].clamp(value)));
        self
    }

    /// Changes parameter `name` at `seconds` into the render, clamped as
    /// [`Harness::param`] clamps it. The harness ends a block there, so the
    /// change lands on the same frame at every block size.
    #[track_caller]
    pub fn param_at(mut self, seconds: f64, name: &str, value: f32) -> Self {
        let index = self.param_named(name);
        self.param_changes
            .push((seconds, index, self.descriptor.params[index].clamp(value)));
        self
    }

    /// The index of parameter `name`. Panics with the names it has.
    #[track_caller]
    fn param_named(&self, name: &str) -> usize {
        self.descriptor.param_index(name).unwrap_or_else(|| {
            panic!(
                "{}: no parameter `{name}`; it has {:?}",
                self.descriptor.id,
                self.descriptor
                    .params
                    .iter()
                    .map(|param| param.name)
                    .collect::<Vec<_>>()
            )
        })
    }

    /// MIDI events. Each event's `frame` counts from the start of the render.
    pub fn midi(mut self, events: impl IntoIterator<Item = MidiEvent>) -> Self {
        self.midi.extend(events);
        self
    }

    /// The transport the module sees, starting at position 0. Default:
    /// stopped, at 120 BPM, in 4/4.
    pub fn transport(mut self, settings: TransportSettings) -> Self {
        self.transport = settings;
        self
    }

    /// Changes the transport at `seconds` into the render. The position stays
    /// where it is. A tempo or running change is not a jump, and a time signature
    /// change is a jump for one call (clock spec, rule 10). The harness ends a
    /// block there, so the change lands on the same frame at every block size.
    pub fn transport_at(mut self, seconds: f64, settings: TransportSettings) -> Self {
        self.transport_changes.push((seconds, settings));
        self
    }

    /// Largest allowed difference from the reference, per sample. Default
    /// [`DEFAULT_TOLERANCE`].
    pub fn tolerance(mut self, tolerance: f32) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Allows ports and parameters with no label or description, for a
    /// quick experiment. By default every render fails on one, because the
    /// app and help pages show them to users.
    pub fn allow_undocumented(mut self) -> Self {
        self.allow_undocumented = true;
        self
    }

    /// Length of the render in frames.
    pub fn frame_count(&self) -> usize {
        match self.length {
            Length::Frames(frames) => frames,
            Length::Seconds(seconds) => (seconds * f64::from(self.sample_rate)).round() as usize,
        }
    }

    /// Renders in blocks of [`MAX_BLOCK`] frames.
    #[track_caller]
    pub fn render(&self) -> Render {
        self.render_with_block(MAX_BLOCK)
    }

    /// Renders in blocks of `block` frames, from 1 to [`MAX_BLOCK`].
    #[track_caller]
    pub fn render_with_block(&self, block: usize) -> Render {
        assert!(
            (1..=MAX_BLOCK).contains(&block),
            "block size {block} is not between 1 and {MAX_BLOCK}"
        );
        let descriptor = self.descriptor;
        if !self.allow_undocumented {
            let missing = descriptor.undocumented();
            if !missing.is_empty() {
                let list: Vec<String> = missing.iter().map(ToString::to_string).collect();
                panic!(
                    "{}: {}. Give every port and parameter a short label and a one-sentence description, or call \
                     `.allow_undocumented()` for a quick experiment.",
                    descriptor.id,
                    list.join("; ")
                );
            }
        }
        let (inputs, connected) = self.input_buffers();
        self.run(block, &inputs, &connected)
    }

    /// Every input's samples for the whole render, and which inputs have a
    /// cable. An unconnected input reads its port default.
    fn input_buffers(&self) -> (Vec<Vec<f32>>, Vec<bool>) {
        let frames = self.frame_count();
        let transport = self.transport_spans();
        let mut inputs: Vec<Vec<f32>> = self
            .descriptor
            .inputs
            .iter()
            .zip(&self.inputs)
            .map(|(port, signal)| match signal {
                Some(signal) => signal.render(frames, self.sample_rate),
                None => default_signal(port.default, &transport, frames),
            })
            .collect();
        let layout = self.input_layout();
        inputs.resize_with(layout.channels(), || vec![0.0; frames]);
        for (&port, (width, signals)) in &self.bundle_inputs {
            for voice in 0..width.voices {
                for side in 0..width.sides {
                    inputs[layout.index(port, voice, side)] =
                        signals[voice * width.sides + side].render(frames, self.sample_rate);
                }
            }
        }
        let connected = self
            .inputs
            .iter()
            .enumerate()
            .map(|(port, signal)| signal.is_some() || self.bundle_inputs.contains_key(&port))
            .collect();
        (inputs, connected)
    }

    /// The frame of a change at `seconds`, rounded as [`Harness::seconds`] is.
    fn frame_at(&self, seconds: f64) -> usize {
        (seconds * f64::from(self.sample_rate)).round() as usize
    }

    /// The parameter changes as `(frame, index, value)`, by frame, and in the
    /// order they were added on one frame.
    fn scheduled_params(&self) -> Vec<(usize, usize, f32)> {
        let mut changes: Vec<_> = self
            .param_changes
            .iter()
            .map(|&(seconds, index, value)| (self.frame_at(seconds), index, value))
            .collect();
        changes.sort_by_key(|&(frame, ..)| frame);
        changes
    }

    /// The transport over the render, in frame order: the first span from frame
    /// 0, and one for each frame with a change.
    fn transport_spans(&self) -> Vec<TransportSpan> {
        let mut spans = vec![TransportSpan {
            start: 0,
            settings: self.transport,
            info: TransportInfo::new(self.sample_rate, self.transport, 0.0),
        }];
        let mut changes: Vec<_> = self
            .transport_changes
            .iter()
            .map(|&(seconds, settings)| (self.frame_at(seconds), settings))
            .collect();
        changes.sort_by_key(|&(frame, _)| frame);
        for (start, settings) in changes {
            let last = spans[spans.len() - 1];
            let before = last.info.later(start - last.start);
            let moved = settings.time_signature != last.settings.time_signature;
            // Two changes on one frame are one change, so the jump stays.
            let jump = moved || (start == last.start && last.info.jumped());
            let info = TransportInfo::new(self.sample_rate, settings, before.position()).with_jump(jump);
            if start == last.start {
                spans.pop();
            }
            spans.push(TransportSpan { start, settings, info });
        }
        spans
    }

    /// Panics if the harness has scheduled changes. A reset or a new copy cannot
    /// replay a schedule that has already run.
    #[track_caller]
    fn refuse_scheduled_changes(&self, check: &str) {
        assert!(
            self.param_changes.is_empty() && self.transport_changes.is_empty(),
            "{}: `{check}` does not support `.param_at` or `.transport_at`: a reset or a new copy cannot replay a \
             schedule that has already run",
            self.descriptor.id
        );
    }

    /// A new instance with the harness's parameter values, set up as the
    /// `Controller` sets one up: built, given its defaults and then its
    /// values, and `reset()` once, so its smoothing starts at the values
    /// (live patching spec, rule 7).
    fn build(&self) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = self.descriptor.role else {
            unreachable!("checked in Harness::new")
        };
        let mut module = create(&ModuleConfig {
            sample_rate: self.sample_rate,
            max_block: MAX_BLOCK,
        });
        for (index, param) in self.descriptor.params.iter().enumerate() {
            module.set_param(index, param.default);
        }
        for &(index, value) in &self.params {
            module.set_param(index, value);
        }
        module.reset();
        module
    }

    /// Checks that `reset()` returns a module to its start (module test
    /// harness, Reset returns a module to its start). It renders a new
    /// instance, calls `reset()`, renders again with the same inputs, MIDI, and
    /// transport, and panics unless the second render is bit-identical to the
    /// first. A module fails when `reset()` leaves a parameter gliding, or
    /// keeps state that `process` changed.
    #[track_caller]
    pub fn check_reset(&self) {
        self.refuse_scheduled_changes("check_reset");
        let (inputs, connected) = self.input_buffers();
        let mut module = self.build();
        let first = self.run_module_ref(module.as_mut(), MAX_BLOCK, &inputs, &connected);
        module.reset();
        let second = self.run_module_ref(module.as_mut(), MAX_BLOCK, &inputs, &connected);
        for (port, (a, b)) in first.outputs.iter().zip(&second.outputs).enumerate() {
            if let Some(frame) = a.iter().zip(b).position(|(a, b)| a.to_bits() != b.to_bits()) {
                panic!(
                    "{}: output `{}` at frame {frame} is {} on a new instance, but {} after a render and `reset()`. \
                     `reset()` must snap smoothing to its targets and clear the state that `process` changes \
                     (docs/design/module-test-harness.md, Reset returns a module to its start).",
                    self.descriptor.id,
                    self.descriptor
                        .outputs
                        .get(port)
                        .map_or("bundle channel", |port| port.name),
                    a[frame],
                    b[frame],
                );
            }
        }
    }

    /// Runs a new instance of the module over `inputs` in blocks of `block`
    /// frames.
    #[track_caller]
    fn run(&self, block: usize, inputs: &[Vec<f32>], connected: &[bool]) -> Render {
        self.run_module(self.build(), block, inputs, connected)
    }

    /// Runs `module` over `inputs` in blocks of `block` frames.
    #[track_caller]
    fn run_module(&self, mut module: Box<dyn Module>, block: usize, inputs: &[Vec<f32>], connected: &[bool]) -> Render {
        self.run_module_ref(module.as_mut(), block, inputs, connected)
    }

    /// Runs `module`, which the caller keeps, over `inputs` in blocks of
    /// `block` frames.
    #[track_caller]
    fn run_module_ref(&self, module: &mut dyn Module, block: usize, inputs: &[Vec<f32>], connected: &[bool]) -> Render {
        let descriptor = self.descriptor;
        let frames = self.frame_count();
        for event in &self.midi {
            assert!(
                (event.frame as usize) < frames,
                "{}: a MIDI event at frame {} is past the end of the render ({frames} frames)",
                descriptor.id,
                event.frame
            );
        }
        let last_change = self
            .param_changes
            .iter()
            .map(|&(seconds, ..)| seconds)
            .chain(self.transport_changes.iter().map(|&(seconds, _)| seconds))
            .map(|seconds| self.frame_at(seconds))
            .max();
        if let Some(frame) = last_change {
            assert!(
                frame < frames,
                "{}: a scheduled change at frame {frame} is past the end of the render ({frames} frames)",
                descriptor.id
            );
        }
        let mut outputs = vec![vec![0.0; frames]; self.output_layout(None).channels()];
        self.run_span(module, block, inputs, connected, 0..frames, &mut outputs);
        Render {
            descriptor,
            sample_rate: self.sample_rate,
            outputs,
            layout: self.output_layout(None),
        }
    }

    /// Runs `module` over the frames in `span` of the render, in blocks of
    /// `block` frames, with the inputs, MIDI, and transport of those frames.
    /// Writes the same frames of `outputs`.
    fn run_span(
        &self,
        module: &mut dyn Module,
        block: usize,
        inputs: &[Vec<f32>],
        connected: &[bool],
        span: std::ops::Range<usize>,
        outputs: &mut [Vec<f32>],
    ) {
        let transport = self.transport_spans();
        let params = self.scheduled_params();
        let frames = span.end;
        let mut block_midi = Vec::with_capacity(self.midi.len());
        let mut start = span.start;
        while start < frames {
            for &(_, index, value) in params.iter().filter(|&&(frame, ..)| frame == start) {
                module.set_param(index, value);
            }
            // A block ends at the next change, so the change lands on its frame
            // at every block size.
            let next_change = params
                .iter()
                .map(|&(frame, ..)| frame)
                .chain(transport.iter().map(|span| span.start))
                .filter(|&frame| frame > start)
                .min()
                .unwrap_or(frames);
            let len = block.min(next_change.min(frames) - start);
            let end = start + len;
            block_midi.clear();
            block_midi.extend(
                self.midi
                    .iter()
                    .filter(|event| (start..end).contains(&(event.frame as usize)))
                    .map(|event| MidiEvent {
                        frame: event.frame - start as u32,
                        ..*event
                    }),
            );
            let block_inputs: Vec<&[f32]> = inputs.iter().map(|input| &input[start..end]).collect();
            if self
                .descriptor
                .inputs
                .iter()
                .chain(self.descriptor.outputs)
                .any(|port| port.channels == loom_core::Channels::Voices)
            {
                let maximum = self.output_layout(None);
                let active = self.output_layout(Some(start));
                let mut wanted = vec![false; maximum.channels()];
                for port in 0..active.ports() {
                    let width = active.width(port);
                    for voice in 0..width.voices {
                        for side in 0..width.sides {
                            wanted[maximum.index(port, voice, side)] = true;
                        }
                    }
                }
                let mut block_outputs: Vec<&mut [f32]> = outputs
                    .iter_mut()
                    .enumerate()
                    .filter(|(index, _)| wanted[*index])
                    .map(|(_, output)| &mut output[start..end])
                    .collect();
                let ctx = loom_core::ProcessContext::new(self.sample_rate, len, &block_midi)
                    .with_transport(transport_at_frame(&transport, start));
                testing::run_bundles(
                    module,
                    &ctx,
                    &self.input_layout(),
                    &block_inputs,
                    connected,
                    &active,
                    &mut block_outputs,
                );
            } else {
                let mut block_outputs: Vec<&mut [f32]> =
                    outputs.iter_mut().map(|output| &mut output[start..end]).collect();
                testing::run_with_transport(
                    module,
                    self.sample_rate,
                    transport_at_frame(&transport, start),
                    &block_inputs,
                    connected,
                    &mut block_outputs,
                    &block_midi,
                );
            }
            start = end;
        }
    }

    /// Runs a new instance for the first `at` frames, then a second new
    /// instance takes its state with [`Module::copy_state_from`], and both
    /// run on to the end, in blocks of `block` frames. Returns whether the copy
    /// took the state, and the two outputs from frame `at`: the running
    /// instance's first.
    #[track_caller]
    fn copied_at(&self, at: usize, block: usize) -> (bool, Vec<Vec<f32>>, Vec<Vec<f32>>) {
        self.refuse_scheduled_changes("check_copy_state");
        let frames = self.frame_count();
        assert!(
            at < frames,
            "{}: the copy at frame {at} is past the end of the render ({frames} frames)",
            self.descriptor.id
        );
        let (inputs, connected) = self.input_buffers();
        let outputs = || vec![vec![0.0; frames]; self.output_layout(None).channels()];
        let mut running = self.build();
        let mut first = outputs();
        self.run_span(running.as_mut(), block, &inputs, &connected, 0..at, &mut first);
        let mut copy = self.build();
        let copied = copy.copy_state_from(&*running);
        // Display values are state too: a light must not lose a pending flash.
        let displays = self.descriptor.displays.len();
        let (mut shown, mut shown_copy) = (vec![0.0; displays], vec![0.0; displays]);
        running.displays(&mut shown);
        copy.displays(&mut shown_copy);
        for (index, (a, b)) in shown.iter().zip(&shown_copy).enumerate() {
            assert!(
                a.to_bits() == b.to_bits(),
                "{}: display `{}` is {a} on the running instance, but {b} on the copy that took its state (frame {at}, \
                 block size {block}, `copy_state_from` returned {copied}).",
                self.descriptor.id,
                self.descriptor.displays[index].name,
            );
        }
        let mut second = outputs();
        self.run_span(running.as_mut(), block, &inputs, &connected, at..frames, &mut first);
        self.run_span(copy.as_mut(), block, &inputs, &connected, at..frames, &mut second);
        let from = |outputs: Vec<Vec<f32>>| outputs.into_iter().map(|o| o[at..].to_vec()).collect();
        (copied, from(first), from(second))
    }

    /// Checks [`Module::copy_state_from`] (ADR-0022, stereo spec rules 25
    /// and 26). At each of [`BLOCK_SIZES`], it runs a new instance for `at`
    /// frames, copies its state into a second new instance, runs both on to
    /// the end of the render, and panics unless their outputs are bit-identical
    /// from frame `at`. Returns whether the module copied its state. A module
    /// with no state passes without copying; a module with state that does not
    /// copy it fails. Not for seeded modules: see
    /// [`Harness::check_copy_state_repeatable`].
    #[track_caller]
    pub fn check_copy_state(&self, at: usize) -> bool {
        let mut copied = false;
        for block in BLOCK_SIZES {
            let (took, running, copy) = self.copied_at(at, block);
            copied = took;
            for (port, (a, b)) in running.iter().zip(&copy).enumerate() {
                if let Some(frame) = a.iter().zip(b).position(|(a, b)| a.to_bits() != b.to_bits()) {
                    panic!(
                        "{}: output `{}` at frame {} is {} on the running instance, but {} on the copy that took its \
                         state (block size {block}, `copy_state_from` returned {copied}). A module with state must \
                         copy all of it (docs/design/stereo.md, rules 25 and 26).",
                        self.descriptor.id,
                        self.descriptor
                            .outputs
                            .get(port)
                            .map_or("bundle channel", |port| port.name),
                        at + frame,
                        a[frame],
                        b[frame],
                    );
                }
            }
        }
        copied
    }

    /// The check for a seeded module (stereo spec, rule 27), which must not
    /// copy its random state: copying at frame `at` twice gives two copies
    /// with bit-identical outputs, and `copy_state_from` returns `true`.
    #[track_caller]
    pub fn check_copy_state_repeatable(&self, at: usize) {
        let (copied, _, first) = self.copied_at(at, MAX_BLOCK);
        let (_, _, second) = self.copied_at(at, MAX_BLOCK);
        assert!(copied, "{}: `copy_state_from` returned false", self.descriptor.id);
        assert!(
            first == second,
            "{}: two copies taken at frame {at} play different outputs",
            self.descriptor.id
        );
    }

    /// Renders with each of [`BLOCK_SIZES`] and panics unless the outputs
    /// are bit-identical and finite. Returns the render in blocks of
    /// [`MAX_BLOCK`]. `docs/learn/block-size-invariance.md` explains why.
    #[track_caller]
    pub fn check_block_sizes(&self) -> Render {
        let reference = self.render();
        reference.assert_finite(MAX_BLOCK);
        for block in BLOCK_SIZES {
            if block == MAX_BLOCK {
                continue;
            }
            let render = self.render_with_block(block);
            render.assert_finite(block);
            for (port, (a, b)) in render.outputs.iter().zip(&reference.outputs).enumerate() {
                if let Some(frame) = a.iter().zip(b).position(|(a, b)| a.to_bits() != b.to_bits()) {
                    panic!(
                        "{}: output `{}` at frame {frame} is {} with block size {block}, but {} with block size \
                         {MAX_BLOCK}. A module must give identical output for any block size (AGENTS.md hard rule 2).",
                        self.descriptor.id,
                        self.descriptor
                            .outputs
                            .get(port)
                            .map_or("bundle channel", |port| port.name),
                        a[frame],
                        b[frame],
                    );
                }
            }
        }
        reference
    }

    /// Renders in blocks of [`MAX_BLOCK`] and compares with the reference
    /// file at `path`, or writes it when `LOOM_UPDATE_REFERENCES=1`.
    #[track_caller]
    pub fn check_reference(&self, path: impl AsRef<Path>) -> Render {
        let render = self.render();
        render.assert_finite(MAX_BLOCK);
        self.compare_or_update(&render, path.as_ref(), reference::update_requested());
        render
    }

    /// [`Harness::check_block_sizes`], then [`Harness::check_reference`].
    #[track_caller]
    pub fn check(&self, path: impl AsRef<Path>) -> Render {
        let render = self.check_block_sizes();
        self.compare_or_update(&render, path.as_ref(), reference::update_requested());
        render
    }

    /// Checks that the module recovers from a non-finite input (#127).
    ///
    /// For each input in turn, and for each of NaN, infinity, and minus
    /// infinity, it renders with that value on the input for
    /// [`BAD_FRAMES`] frames from frame [`BAD_START`] (or the configured
    /// [`Harness::non_finite_start`]), in blocks of 1 frame (as a feedback loop
    /// calls the module) and of [`MAX_BLOCK`]. The input
    /// counts as connected. Every other frame and input reads what the
    /// render reads without the check: its signal, or its port default. It
    /// panics unless every output is finite again from [`RECOVERY_SECONDS`]
    /// after the bad frames to the end of the render.
    ///
    /// Only inputs get bad values. Parameters cannot: the engine fills a
    /// modulated parameter through `ParamSpec::denormalize`, which turns NaN
    /// into the default and clamps infinity.
    ///
    /// A module passes when it ignores a non-finite input or recovers from
    /// it. It fails when it keeps the value in its state, because the patch
    /// then stays broken after its input is sane again. The render must be
    /// long enough to show that: more than the configured start + [`BAD_FRAMES`]
    /// frames plus [`RECOVERY_SECONDS`] of frames, and longer for a module that holds
    /// samples, such as a delay, so a held bad value would come out again.
    #[track_caller]
    pub fn check_non_finite_recovery(&self) {
        let frames = self.frame_count();
        let start = self.non_finite_start;
        let recovery_frames = (RECOVERY_SECONDS * f64::from(self.sample_rate)).ceil() as usize;
        let clean_from = start + BAD_FRAMES + recovery_frames;
        assert!(
            frames > clean_from,
            "{}: the render is {frames} frames long, but the non-finite check needs more than {clean_from}",
            self.descriptor.id
        );
        let (clean, connected) = self.input_buffers();
        for (port, spec) in self.descriptor.inputs.iter().enumerate() {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut inputs = clean.clone();
                inputs[port][start..start + BAD_FRAMES].fill(bad);
                let mut connected = connected.clone();
                connected[port] = true;
                // Block size 1 is how a feedback loop calls the module.
                for block in [1, MAX_BLOCK] {
                    let render = self.run(block, &inputs, &connected);
                    for (output, samples) in render.outputs.iter().enumerate() {
                        if let Some(frame) = samples[clean_from..].iter().position(|value| !value.is_finite()) {
                            let frame = clean_from + frame;
                            panic!(
                                "{}: after {bad} on input `{}` (frames {start} to {}), output `{}` is still {} at \
                             frame {frame} (block size {block}). A module must ignore a non-finite input or recover \
                             from it, not keep it in its state.",
                                self.descriptor.id,
                                spec.name,
                                start + BAD_FRAMES - 1,
                                self.descriptor.outputs[render.layout.port_of(output)].name,
                                samples[frame],
                            );
                        }
                    }
                }
            }
        }
    }

    /// Checks that a module survives huge finite inputs (#552). Cables are never
    /// clipped (ADR-0007), so a feedback patch can send these. For each input in
    /// turn, it holds `f32::MAX`, then `-f32::MAX`, then one flipping to the other
    /// at [`EXTREME_FLIP`], from [`EXTREME_START`] for [`EXTREME_FRAMES`]
    /// frames. It panics unless every output is finite for
    /// the whole render, matches a clean render from the end of the settling
    /// time up to [`EXTREME_NEXT_EDGE`], and has a normal size after that. A
    /// phase output matches by its speed, not its value.
    ///
    /// The render must last [`EXTREME_RENDER`] frames at 48 kHz, on inputs of
    /// [`Signal::extreme_busy`] or their defaults.
    #[track_caller]
    pub fn check_extreme_recovery(&self) {
        self.check_extreme(ExtremeTarget::Each, ExtremeLevel::Recovers);
    }

    /// [`Harness::check_extreme_recovery`] for one input.
    #[track_caller]
    pub fn check_extreme_recovery_on(&self, input: &str) {
        self.check_extreme(ExtremeTarget::One(self.extreme_port(input)), ExtremeLevel::Recovers);
    }

    /// The part of [`Harness::check_extreme_recovery`] that does not compare
    /// with a clean render, for one input. It is for an input where a huge value
    /// is a real event, so the module rightly diverges from a clean render, such
    /// as a counter's trigger. The output must still stay finite and return to a
    /// normal size.
    #[track_caller]
    pub fn check_extreme_stays_bounded_on(&self, input: &str) {
        self.check_extreme(ExtremeTarget::One(self.extreme_port(input)), ExtremeLevel::Bounded);
    }

    /// [`Harness::check_extreme_recovery`] with the huge value on every input at
    /// once. A bug that needs two huge inputs together, such as a product that
    /// overflows, never shows with one input at a time.
    #[track_caller]
    pub fn check_extreme_recovery_all(&self) {
        self.check_extreme(ExtremeTarget::AllAtOnce, ExtremeLevel::Recovers);
    }

    /// [`Harness::check_extreme_stays_bounded_on`] with the huge value on every
    /// input at once, for a module with a stated exception.
    #[track_caller]
    pub fn check_extreme_stays_bounded_all(&self) {
        self.check_extreme(ExtremeTarget::AllAtOnce, ExtremeLevel::Bounded);
    }

    /// Only the finite part of [`Harness::check_extreme_recovery`], each input in
    /// turn. It is for a module with its parameters turned up, which may rightly
    /// ring for a long time.
    #[track_caller]
    pub fn check_extreme_finite(&self) {
        self.check_extreme(ExtremeTarget::Each, ExtremeLevel::Finite);
    }

    /// [`Harness::check_extreme_finite`] with the huge value on every input at
    /// once.
    #[track_caller]
    pub fn check_extreme_finite_all(&self) {
        self.check_extreme(ExtremeTarget::AllAtOnce, ExtremeLevel::Finite);
    }

    /// The index of input `name`, for the extreme check.
    #[track_caller]
    fn extreme_port(&self, name: &str) -> usize {
        self.descriptor
            .input_index(name)
            .unwrap_or_else(|| panic!("{}: there is no input `{name}`", self.descriptor.id))
    }

    /// Runs the extreme check on `target`, requiring `level` of the outputs.
    #[track_caller]
    fn check_extreme(&self, target: ExtremeTarget, level: ExtremeLevel) {
        let id = self.descriptor.id;
        let frames = self.frame_count();
        assert!(
            frames >= EXTREME_RENDER,
            "{id}: the render is {frames} frames long, but the extreme check needs {EXTREME_RENDER}"
        );
        let hold_end = EXTREME_START + EXTREME_FRAMES;
        let compare_from = hold_end + EXTREME_SETTLE;
        let (clean_inputs, connected) = self.input_buffers();
        let base = self.run(MAX_BLOCK, &clean_inputs, &connected);
        for (ports, name) in self.extreme_runs(target) {
            // A clean render differs only if the check adds a cable.
            let added = ports.iter().any(|&port| !connected[port]);
            let mut connected = connected.clone();
            for &port in &ports {
                connected[port] = true;
            }
            let own = added.then(|| self.run(MAX_BLOCK, &clean_inputs, &connected));
            let clean = own.as_ref().unwrap_or(&base);
            for (extreme, flipped) in [(f32::MAX, f32::MAX), (-f32::MAX, -f32::MAX), (f32::MAX, -f32::MAX)] {
                let mut inputs = clean_inputs.clone();
                for &port in &ports {
                    inputs[port][EXTREME_START..EXTREME_FLIP].fill(extreme);
                    inputs[port][EXTREME_FLIP..hold_end].fill(flipped);
                }
                let render = self.run(MAX_BLOCK, &inputs, &connected);
                let what = |output: usize| {
                    format!(
                        "{id}: after {extreme:e} then {flipped:e} on {name}, output `{}`",
                        self.descriptor.outputs[render.layout.port_of(output)].name
                    )
                };
                for (output, (samples, clean)) in render.outputs.iter().zip(&clean.outputs).enumerate() {
                    if let Some(frame) = samples.iter().position(|value| !value.is_finite()) {
                        panic!("{} is {} at frame {frame}: not finite", what(output), samples[frame]);
                    }
                    if level == ExtremeLevel::Finite {
                        continue;
                    }
                    let peak = clean.iter().fold(1.0f32, |peak, value| peak.max(value.abs()));
                    if level == ExtremeLevel::Recovers {
                        let phase = self.descriptor.outputs[render.layout.port_of(output)].signal == SignalType::Phase;
                        let frame = (compare_from..EXTREME_NEXT_EDGE).find(|&frame| {
                            if phase {
                                // A captured value may rightly leave a phase at
                                // another offset, but it must move at the same
                                // speed, the shorter way round the cycle.
                                let step = |s: &[f32]| s[frame] - s[frame - 1];
                                let gap = step(samples) - step(clean);
                                (gap - gap.round()).abs() > 1e-6
                            } else {
                                (samples[frame] - clean[frame]).abs() > peak * 1e-3
                            }
                        });
                        if let Some(frame) = frame {
                            panic!(
                                "{} is {} at frame {frame}, but {} on a clean render: it did not recover",
                                what(output),
                                samples[frame],
                                clean[frame]
                            );
                        }
                    }
                    if let Some(frame) = samples[EXTREME_NEXT_EDGE..].iter().position(|v| v.abs() > peak * 10.0) {
                        let frame = EXTREME_NEXT_EDGE + frame;
                        panic!(
                            "{} is {} at frame {frame}, but a clean render peaks at {peak}: it stayed huge",
                            what(output),
                            samples[frame]
                        );
                    }
                }
            }
        }
    }

    /// The runs `target` asks for: the inputs that take the huge value together,
    /// and how a failure names them. A module with no inputs has no run.
    fn extreme_runs(&self, target: ExtremeTarget) -> Vec<(Vec<usize>, String)> {
        let inputs = self.descriptor.inputs;
        let one = |port: usize| (vec![port], format!("input `{}`", inputs[port].name));
        match target {
            ExtremeTarget::Each => (0..inputs.len()).map(one).collect(),
            ExtremeTarget::One(port) => vec![one(port)],
            ExtremeTarget::AllAtOnce if inputs.is_empty() => Vec::new(),
            ExtremeTarget::AllAtOnce => vec![((0..inputs.len()).collect(), "every input at once".to_string())],
        }
    }

    #[track_caller]
    fn compare_or_update(&self, render: &Render, path: &Path, update: bool) {
        if update {
            reference::write(&reference::Channels::of(render), path);
        } else {
            reference::compare(&reference::Channels::of(render), path, self.tolerance);
        }
    }
}

/// What an unconnected input reads, for the whole render. Computed from the
/// same anchor as the engine's shared buffers, so it is the same value, bit
/// for bit.
fn default_signal(default: PortDefault, transport: &[TransportSpan], frames: usize) -> Vec<f32> {
    let at = |frame: usize| transport_at_frame(transport, frame);
    match default {
        PortDefault::Value(value) => vec![value; frames],
        PortDefault::OwnPhase => vec![0.0; frames],
        PortDefault::BarPhase => (0..frames).map(|frame| at(frame).bar_phase_at(0)).collect(),
        PortDefault::BeatPhase => (0..frames).map(|frame| at(frame).beat_phase_at(0)).collect(),
        PortDefault::Playing => (0..frames)
            .map(|frame| if at(frame).playing() { 1.0 } else { 0.0 })
            .collect(),
    }
}

/// The outputs of one render.
pub struct Render {
    descriptor: &'static ModuleDescriptor,
    sample_rate: f32,
    outputs: Vec<Vec<f32>>,
    layout: testing::BundleLayout,
}

impl Render {
    /// Output `name`. Panics if the module has no such output.
    #[track_caller]
    pub fn output(&self, name: &str) -> &[f32] {
        let Some(index) = self.descriptor.outputs.iter().position(|port| port.name == name) else {
            panic!("{}: no output `{name}`", self.descriptor.id);
        };
        &self.outputs[index]
    }

    /// One zero-based voice and side of an output bundle.
    pub fn voice_output(&self, name: &str, voice: usize, side: usize) -> &[f32] {
        let port = self.descriptor.output_index(name).expect("output exists");
        &self.outputs[self.layout.index(port, voice, side)]
    }

    /// Primary outputs in descriptor order, followed by extra voices/sides
    /// grouped by port. Use `voice_output` to address a particular channel.
    pub fn outputs(&self) -> &[Vec<f32>] {
        &self.outputs
    }

    /// Length in frames.
    pub fn frames(&self) -> usize {
        self.outputs.first().map_or(0, Vec::len)
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    #[track_caller]
    fn assert_finite(&self, block: usize) {
        for (port, output) in self.outputs.iter().enumerate() {
            if let Some(frame) = output.iter().position(|value| !value.is_finite()) {
                panic!(
                    "{}: output `{}` is {} at frame {frame} (block size {block})",
                    self.descriptor.id,
                    self.descriptor
                        .outputs
                        .get(port)
                        .map_or("bundle channel", |port| port.name),
                    output[frame]
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
