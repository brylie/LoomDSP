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
//! #     id: "doc.sine", version: 1, name: "Sine", category: "Doc", description: "",
//! #     inputs: &[], outputs: &[], params: &[], role: loom_core::ModuleRole::UnitDelay,
//! #     panel: loom_core::Panel::EMPTY, displays: &[],
//! # };
//! Harness::new(&SINE)
//!     .seconds(0.05)
//!     .input("phase", Signal::phase(440.0))
//!     .check(reference!("sine"));
//! ```
//!
//! [`modules`] holds test-only modules (IDs under `test.`) with every
//! panel widget, parameter mapping, and output polarity, for tests that the
//! built-in modules do not cover.
//!
//! [`analysis`] measures recorded audio (peak, clipping, steps, DC), for
//! app tests and `loom analyze` (`docs/design/audio-qa.md`).
//!
//! References are 32-bit float WAV files, so a reviewer can listen to them.
//! Create or update them with `LOOM_UPDATE_REFERENCES=1 cargo test`.
//!
//! This crate reads and writes files, so it is for tests and developer
//! tools on the desktop only (the app uses [`modules`] for
//! `--test-modules`). The engine crates never depend on it.

#![warn(missing_docs)]

pub mod analysis;
pub mod modules;
mod reference;
mod signal;

pub use reference::UPDATE_VAR;
pub use signal::Signal;

use loom_core::{
    MidiEvent, ModuleConfig, ModuleDescriptor, ModuleRole, PortDefault, TransportInfo, TransportSettings, testing,
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

/// Frames a module may take to give finite output again after the
/// non-finite input ends, in [`Harness::check_non_finite_recovery`]. About
/// 10 ms at 48 kHz.
pub const RECOVERY_FRAMES: usize = 480;

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

    /// Runs a new instance of the module over `inputs` in blocks of `block`
    /// frames.
    #[track_caller]
    fn run(&self, block: usize, inputs: &[Vec<f32>], connected: &[bool]) -> Render {
        let descriptor = self.descriptor;
        let frames = self.frame_count();
        let ModuleRole::Dsp(create) = descriptor.role else {
            unreachable!("checked in Harness::new")
        };

        // A new instance, set up as `Controller::add_module` sets it up.
        let mut module = create(&ModuleConfig {
            sample_rate: self.sample_rate,
            max_block: MAX_BLOCK,
        });
        for (index, param) in descriptor.params.iter().enumerate() {
            module.set_param(index, param.default);
        }
        for &(index, value) in &self.params {
            module.set_param(index, value);
        }

        let transport = TransportInfo::new(self.sample_rate, self.transport, 0.0);
        for event in &self.midi {
            assert!(
                (event.frame as usize) < frames,
                "{}: a MIDI event at frame {} is past the end of the render ({frames} frames)",
                descriptor.id,
                event.frame
            );
        }

        let mut outputs = vec![vec![0.0; frames]; descriptor.outputs.len()];
        let mut block_midi = Vec::with_capacity(self.midi.len());
        let mut start = 0;
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
                module.as_mut(),
                self.sample_rate,
                transport.later(start),
                &block_inputs,
                connected,
                &mut block_outputs,
                &block_midi,
            );
            start = end;
        }

        Render {
            descriptor,
            sample_rate: self.sample_rate,
            outputs,
        }
    }

    /// Renders with each of [`BLOCK_SIZES`] and panics unless the outputs
    /// are bit-identical and finite. Returns the render in blocks of
    /// [`MAX_BLOCK`].
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
    /// panics unless every output is finite again from [`RECOVERY_FRAMES`]
    /// after the bad frames to the end of the render.
    ///
    /// Only inputs get bad values. Parameters cannot: the engine fills a
    /// modulated parameter through `ParamSpec::denormalize`, which turns NaN
    /// into the default and clamps infinity.
    ///
    /// A module passes when it ignores a non-finite input or recovers from
    /// it. It fails when it keeps the value in its state, because the patch
    /// then stays broken after its input is sane again. The render must be
    /// long enough to show that: at least [`BAD_START`] + [`BAD_FRAMES`] +
    /// [`RECOVERY_FRAMES`] frames, and longer for a module that holds
    /// samples, such as a delay, so a held bad value would come out again.
    #[track_caller]
    pub fn check_non_finite_recovery(&self) {
        let frames = self.frame_count();
        let clean_from = BAD_START + BAD_FRAMES + RECOVERY_FRAMES;
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

    #[track_caller]
    fn compare_or_update(&self, render: &Render, path: &Path, update: bool) {
        if update {
            reference::write(render, path);
        } else {
            reference::compare(render, path, self.tolerance);
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
