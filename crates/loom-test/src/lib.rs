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
//! #     panel: loom_core::Panel::EMPTY, displays: &[],
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
/// it, so it has state to lose.
pub const BAD_START: usize = 1_000;

/// How many frames the non-finite input lasts in
/// [`Harness::check_non_finite_recovery`].
pub const BAD_FRAMES: usize = 8;

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
    params: Vec<(usize, f32)>,
    midi: Vec<MidiEvent>,
    transport: TransportSettings,
    tolerance: f32,
    allow_undocumented: bool,
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
            params: Vec::new(),
            midi: Vec::new(),
            transport: TransportSettings {
                running: false,
                ..TransportSettings::default()
            },
            tolerance: DEFAULT_TOLERANCE,
            allow_undocumented: false,
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
        self.inputs[index] = Some(signal);
        self
    }

    /// Sets parameter `name`, clamped to its range as
    /// `Controller::set_param` clamps it.
    #[track_caller]
    pub fn param(mut self, name: &str, value: f32) -> Self {
        let Some(index) = self.descriptor.param_index(name) else {
            panic!(
                "{}: no parameter `{name}`; it has {:?}",
                self.descriptor.id,
                self.descriptor
                    .params
                    .iter()
                    .map(|param| param.name)
                    .collect::<Vec<_>>()
            );
        };
        self.params.push((index, self.descriptor.params[index].clamp(value)));
        self
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
        let transport = TransportInfo::new(self.sample_rate, self.transport, 0.0);
        let inputs = self
            .descriptor
            .inputs
            .iter()
            .zip(&self.inputs)
            .map(|(port, signal)| match signal {
                Some(signal) => signal.render(frames, self.sample_rate),
                None => default_signal(port.default, &transport, frames),
            })
            .collect();
        let connected = self.inputs.iter().map(Option::is_some).collect();
        (inputs, connected)
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
                    self.descriptor.id, self.descriptor.outputs[port].name, a[frame], b[frame],
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
        let mut outputs = vec![vec![0.0; frames]; descriptor.outputs.len()];
        self.run_span(module, block, inputs, connected, 0..frames, &mut outputs);
        Render {
            descriptor,
            sample_rate: self.sample_rate,
            outputs,
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
        let transport = TransportInfo::new(self.sample_rate, self.transport, 0.0);
        let frames = span.end;
        let mut block_midi = Vec::with_capacity(self.midi.len());
        let mut start = span.start;
        while start < frames {
            let len = block.min(frames - start);
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
            let mut block_outputs: Vec<&mut [f32]> = outputs.iter_mut().map(|output| &mut output[start..end]).collect();
            testing::run_with_transport(
                module,
                self.sample_rate,
                transport.later(start),
                &block_inputs,
                connected,
                &mut block_outputs,
                &block_midi,
            );
            start = end;
        }
    }

    /// Runs a new instance for the first `at` frames, then a second new
    /// instance takes its state with [`Module::copy_state_from`], and both
    /// run on to the end, in blocks of `block` frames. Returns whether the copy
    /// took the state, and the two outputs from frame `at`: the running
    /// instance's first.
    fn copied_at(&self, at: usize, block: usize) -> (bool, Vec<Vec<f32>>, Vec<Vec<f32>>) {
        let frames = self.frame_count();
        assert!(
            at < frames,
            "{}: the copy at frame {at} is past the end of the render ({frames} frames)",
            self.descriptor.id
        );
        let (inputs, connected) = self.input_buffers();
        let outputs = || vec![vec![0.0; frames]; self.descriptor.outputs.len()];
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
                        self.descriptor.outputs[port].name,
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
                        self.descriptor.id, self.descriptor.outputs[port].name, a[frame], b[frame],
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
    /// [`BAD_FRAMES`] frames from frame [`BAD_START`], in blocks of 1 frame
    /// (as a feedback loop calls the module) and of [`MAX_BLOCK`]. The input
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
    /// long enough to show that: more than [`BAD_START`] + [`BAD_FRAMES`]
    /// frames plus [`RECOVERY_SECONDS`] of frames, and longer for a module that holds
    /// samples, such as a delay, so a held bad value would come out again.
    #[track_caller]
    pub fn check_non_finite_recovery(&self) {
        let frames = self.frame_count();
        let recovery_frames = (RECOVERY_SECONDS * f64::from(self.sample_rate)).ceil() as usize;
        let clean_from = BAD_START + BAD_FRAMES + recovery_frames;
        assert!(
            frames > clean_from,
            "{}: the render is {frames} frames long, but the non-finite check needs more than {clean_from}",
            self.descriptor.id
        );
        let (clean, connected) = self.input_buffers();
        for (port, spec) in self.descriptor.inputs.iter().enumerate() {
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut inputs = clean.clone();
                inputs[port][BAD_START..BAD_START + BAD_FRAMES].fill(bad);
                let mut connected = connected.clone();
                connected[port] = true;
                // Block size 1 is how a feedback loop calls the module.
                for block in [1, MAX_BLOCK] {
                    let render = self.run(block, &inputs, &connected);
                    for (output, samples) in render.outputs.iter().enumerate() {
                        if let Some(frame) = samples[clean_from..].iter().position(|value| !value.is_finite()) {
                            let frame = clean_from + frame;
                            panic!(
                                "{}: after {bad} on input `{}` (frames {BAD_START} to {}), output `{}` is still {} at \
                             frame {frame} (block size {block}). A module must ignore a non-finite input or recover \
                             from it, not keep it in its state.",
                                self.descriptor.id,
                                spec.name,
                                BAD_START + BAD_FRAMES - 1,
                                self.descriptor.outputs[output].name,
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
        self.check_extreme(true);
    }

    /// The part of [`Harness::check_extreme_recovery`] that does not compare
    /// with a clean render, for a module that rightly diverges from one after
    /// an extra trigger, such as a counter or a seeded stream. Its output must
    /// still stay finite and return to a normal size.
    #[track_caller]
    pub fn check_extreme_stays_bounded(&self) {
        self.check_extreme(false);
    }

    /// Runs the extreme check, comparing with a clean render if `compare`.
    #[track_caller]
    fn check_extreme(&self, compare: bool) {
        let id = self.descriptor.id;
        let frames = self.frame_count();
        assert!(
            frames >= EXTREME_RENDER,
            "{id}: the render is {frames} frames long, but the extreme check needs {EXTREME_RENDER}"
        );
        let hold_end = EXTREME_START + EXTREME_FRAMES;
        let compare_from = hold_end + EXTREME_SETTLE;
        let (clean_inputs, connected) = self.input_buffers();
        for (port, spec) in self.descriptor.inputs.iter().enumerate() {
            let mut connected = connected.clone();
            connected[port] = true;
            let clean = self.run(MAX_BLOCK, &clean_inputs, &connected);
            for (extreme, flipped) in [(f32::MAX, f32::MAX), (-f32::MAX, -f32::MAX), (f32::MAX, -f32::MAX)] {
                let mut inputs = clean_inputs.clone();
                inputs[port][EXTREME_START..EXTREME_FLIP].fill(extreme);
                inputs[port][EXTREME_FLIP..hold_end].fill(flipped);
                let render = self.run(MAX_BLOCK, &inputs, &connected);
                let what = |output: usize| {
                    format!(
                        "{id}: after {extreme:e} then {flipped:e} on input `{}`, output `{}`",
                        spec.name, self.descriptor.outputs[output].name
                    )
                };
                for (output, (samples, clean)) in render.outputs.iter().zip(&clean.outputs).enumerate() {
                    if let Some(frame) = samples.iter().position(|value| !value.is_finite()) {
                        panic!("{} is {} at frame {frame}: not finite", what(output), samples[frame]);
                    }
                    let peak = clean.iter().fold(1.0f32, |peak, value| peak.max(value.abs()));
                    if compare {
                        let phase = self.descriptor.outputs[output].signal == SignalType::Phase;
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
fn default_signal(default: PortDefault, transport: &TransportInfo, frames: usize) -> Vec<f32> {
    match default {
        PortDefault::Value(value) => vec![value; frames],
        PortDefault::BarPhase => (0..frames).map(|frame| transport.bar_phase_at(frame)).collect(),
        PortDefault::BeatPhase => (0..frames).map(|frame| transport.beat_phase_at(frame)).collect(),
        PortDefault::Playing => vec![if transport.playing() { 1.0 } else { 0.0 }; frames],
    }
}

/// The outputs of one render.
pub struct Render {
    descriptor: &'static ModuleDescriptor,
    sample_rate: f32,
    outputs: Vec<Vec<f32>>,
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

    /// Every output, in descriptor order.
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
                    self.descriptor.id, self.descriptor.outputs[port].name, output[frame]
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
