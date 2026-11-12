//! The module API (FR-SDK-01).
//!
//! Built-in and third-party modules implement the same trait. A module only
//! processes samples. Its ports, parameters, and metadata live in a static
//! [`ModuleDescriptor`](crate::ModuleDescriptor).

use crate::midi::MidiEvent;
use crate::transport::TransportInfo;
use std::any::Any;
use std::marker::PhantomData;

/// Gives every module an [`Any`] view of itself, so
/// [`Module::copy_state_from`] can check the other module's type. A blanket
/// implementation covers every type, so no module writes it.
pub trait AsAny {
    /// This value as [`Any`]. Call it on a `&dyn Module`, not on a
    /// `Box<dyn Module>`: on the box it returns the box itself.
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A DSP module instance.
///
/// Rules for `process` (NFR-RT-01): no allocation, no locks, no system calls,
/// no blocking. The engine may call `process` with any number of frames from
/// 1 to the configured maximum block size. Inside feedback loops it is called
/// one frame at a time (ADR-0002), so results must not depend on how frames
/// are grouped into calls.
pub trait Module: Send + AsAny {
    /// Processes `io.frames()` frames.
    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>);

    /// Sets a parameter by index. Values are already clamped to the range in
    /// the descriptor. Called on the audio thread, so it must be cheap.
    fn set_param(&mut self, index: usize, value: f32) {
        let _ = (index, value);
    }

    /// The value parameter `index` has right now, in the parameter's units,
    /// while the module is still gliding toward the value from
    /// [`Module::set_param`]. `None` when it is not gliding, or when the
    /// module does not smooth that parameter.
    ///
    /// When the first assignment on a smoothed parameter is added, the
    /// engine starts its own glide from this value, so the sound does not
    /// step to the target (modulation spec, rule 9). Called on the audio
    /// thread, so the rules for `process` apply.
    ///
    /// A whole-number parameter reports no glide. The module fades on its own
    /// smoother and reads the fill only for the state it wants. A fill started
    /// part-way would round to the old state and reverse the fade.
    fn param_glide(&self, index: usize) -> Option<f32> {
        let _ = index;
        None
    }

    /// Clears internal state (envelopes, filter memory, oscillator phase)
    /// and snaps parameter smoothing to its targets. The `Controller` calls
    /// it once after a new module's first values are set, so the module
    /// starts at them with no glide (live patching spec, rule 7).
    fn reset(&mut self) {}

    /// Takes the running state of `other`, a module of the same type, so
    /// this one continues exactly where `other` is: filter memory, phase,
    /// envelope stage, parameter smoothing, and buffers. Returns `true` if
    /// it did, and `false` when `other` is another type or the module has
    /// no state to copy (the default). The engine calls it when a module
    /// turns stereo, so the new right-side copy joins the running left one
    /// (ADR-0022, stereo spec rules 25 to 27).
    ///
    /// It runs on the audio thread, so the rules for `process` apply: no
    /// allocation, no locks. Copy buffers into the ones this module already
    /// has; both were built with the same `ModuleConfig`. A module with a
    /// random generator copies everything else and starts its generator
    /// again from its seed, as `reset` does (rule 27). For a module whose
    /// fields are all plain values, [`copy_state`] does the work.
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let _ = other;
        false
    }

    /// Writes the module's display values, one per name in its
    /// descriptor's `displays`, for lights on its panel (ADR-0013). The
    /// engine calls it once after each block, on the audio thread, so the
    /// rules for `process` apply. Display values never change the audio.
    fn displays(&self, values: &mut [f32]) {
        let _ = values;
    }

    /// Only for the Transport module ([`TRANSPORT_MODULE`](crate::TRANSPORT_MODULE)):
    /// the transport changes to make before the next block. The engine
    /// calls it before every block, on the audio thread, so the rules for
    /// `process` apply. `grid` is true when the block starts on the
    /// transport grid ([`TRANSPORT_GRID`](crate::TRANSPORT_GRID)): changes
    /// found on the `run` and `reset` inputs must wait for it (rule 11).
    /// Changes from parameters need not wait: they arrive at buffer starts.
    /// `run_cable` is true when the `run` input has a cable, so the module
    /// knows it before its first `process` call.
    fn transport_request(&mut self, grid: bool, run_cable: bool) -> crate::TransportRequest {
        let _ = (grid, run_cable);
        crate::TransportRequest::default()
    }
}

/// Copies `other` into `this` when it is a `T`, for
/// [`Module::copy_state_from`]. Returns whether it did.
///
/// Use it only for a module whose `Clone::clone_from` does not allocate:
/// one whose fields are plain values, arrays, and smoothers, with no `Vec`
/// or `Box`. The `no_alloc` tests check every built-in module.
pub fn copy_state<T: Module + Clone + 'static>(this: &mut T, other: &dyn Module) -> bool {
    match other.as_any().downcast_ref::<T>() {
        Some(other) => {
            this.clone_from(other);
            true
        }
        None => false,
    }
}

/// Information about the current call to [`Module::process`].
pub struct ProcessContext<'a> {
    /// Sample rate in Hz.
    pub sample_rate: f32,
    pub(crate) midi: &'a [MidiEvent],
    /// Frame index (relative to the buffer the events were stamped against)
    /// of the first frame in this call.
    pub(crate) midi_base: usize,
    pub(crate) frames: usize,
    pub(crate) transport: TransportInfo,
    /// 0 for a module that runs as one, or for its left copy; 1 for its
    /// right copy (ADR-0020).
    pub(crate) channel: u8,
    pub(crate) voice: u8,
    pub(crate) voices: u8,
    pub(crate) audio_input: &'a [crate::audio_input::InputFrame],
    pub(crate) input_channels: usize,
}

impl<'a> ProcessContext<'a> {
    /// Creates a context. Mainly useful for testing modules in isolation.
    /// The transport is stopped at position 0, at 120 BPM, in 4/4.
    pub fn new(sample_rate: f32, frames: usize, midi: &'a [MidiEvent]) -> Self {
        Self {
            sample_rate,
            midi,
            midi_base: 0,
            frames,
            transport: TransportInfo::default(),
            channel: 0,
            voice: 0,
            voices: 1,
            audio_input: &[],
            input_channels: 0,
        }
    }

    /// Sets which channel this call runs, for testing modules in isolation.
    /// Values above 1 count as 1.
    pub fn with_channel(mut self, channel: usize) -> Self {
        self.channel = u8::from(channel > 0);
        self
    }

    /// Which copy of the module this call runs: 0 for a module that runs as
    /// one, or for its left copy, and 1 for its right copy. The engine runs
    /// a module as two copies when stereo reaches one of its `PerChannel`
    /// inputs (ADR-0020). A module with a seed must mix this into the seed,
    /// so its two copies do not give the same values (stereo spec, rule 15).
    pub fn channel(&self) -> usize {
        usize::from(self.channel)
    }

    /// One-based voice index; the original instance is voice 1.
    pub fn voice(&self) -> usize {
        usize::from(self.voice) + 1
    }

    /// Number of voices processed by this module's copies.
    pub fn voice_count(&self) -> usize {
        usize::from(self.voices)
    }

    /// Sets the one-based voice index and count for isolation tests.
    pub fn with_voice(mut self, voice: usize, voices: usize) -> Self {
        self.voices = voices.clamp(1, 16) as u8;
        self.voice = voice.clamp(1, usize::from(self.voices)) as u8 - 1;
        self
    }

    /// The input frames for this call and the number of device channels.
    /// An absent or unavailable stream returns an empty slice and zero.
    pub fn audio_input(&self) -> (&[crate::audio_input::InputFrame], usize) {
        (self.audio_input, self.input_channels)
    }

    /// Replaces the transport, for testing modules in isolation.
    pub fn with_transport(mut self, transport: TransportInfo) -> Self {
        self.transport = transport;
        self
    }

    /// The patch transport for this call (ADR-0015). Inside a feedback loop
    /// each call is one frame, and this describes that frame.
    pub fn transport(&self) -> &TransportInfo {
        &self.transport
    }

    /// MIDI events that fall inside this call, as `(frame, event)`, where
    /// `frame` is relative to the first frame of this call.
    pub fn midi(&self) -> impl Iterator<Item = (usize, &MidiEvent)> + '_ {
        let start = self.midi_base;
        let end = start + self.frames;
        self.midi.iter().filter_map(move |event| {
            let frame = event.frame as usize;
            (frame >= start && frame < end).then(|| (frame - start, event))
        })
    }
}

/// The right-hand buffer of a `Stereo` port. The left-hand buffer is the
/// port's usual pointer.
#[derive(Clone, Copy)]
pub(crate) struct RightPtr {
    pub port: u16,
    pub output: bool,
    pub ptr: *mut f32,
}

/// Access to a module's input and output buffers for one call.
///
/// Inputs are read-only. An unconnected input reads its port's default
/// ([`PortDefault`](crate::PortDefault)): zeros unless the descriptor says
/// otherwise. Use [`PortIo::is_connected`] to choose a fallback (for example
/// a parameter value) when an input has no cable.
pub struct PortIo<'a> {
    inputs: &'a [*const f32],
    outputs: &'a [*mut f32],
    /// Modulated parameters for this call: parameter index and the start of
    /// its per-sample buffer. Empty when nothing on this module is modulated.
    params: &'a [(u16, *const f32)],
    /// Right-hand buffers of `Stereo` ports. A `Stereo` input missing here
    /// reads its left buffer on both sides.
    right: &'a [RightPtr],
    bundles: &'a [BundlePort],
    connected: u64,
    defaulted: u64,
    offset: usize,
    frames: usize,
    _marker: PhantomData<&'a mut f32>,
}

/// One bundle-aware port, indexed directly by input then output port number.
/// Channels are contiguous in voice-then-side order, prepared off the audio thread.
#[derive(Debug)]
pub(crate) struct BundlePort {
    pub width: crate::Width,
    pub channels: Box<[*mut f32]>,
}

impl<'a> PortIo<'a> {
    /// # Safety
    ///
    /// Every pointer must be valid for reads (inputs) or writes (outputs) of
    /// `offset + frames` floats for `'a`. No output may alias any input or
    /// any other output. Each parameter pointer, if any, must be readable for
    /// the same range. Each right-hand pointer must be readable (inputs) or
    /// writable (outputs) for the same range, and a right-hand output must
    /// not alias any other buffer. Bundle entries obey the same bounds and
    /// read/write rules; different output channels must name distinct buffers.
    /// Entries may also name their port's ordinary primary/right buffer.
    /// The bundle table must be empty or contain every input followed by every
    /// output. Each port's channel table has width.voices * width.sides entries,
    /// in voice-then-side order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn new(
        inputs: &'a [*const f32],
        outputs: &'a [*mut f32],
        params: &'a [(u16, *const f32)],
        right: &'a [RightPtr],
        bundles: &'a [BundlePort],
        connected: u64,
        offset: usize,
        frames: usize,
    ) -> Self {
        Self {
            inputs,
            outputs,
            params,
            right,
            bundles,
            connected,
            defaulted: !connected,
            offset,
            frames,
            _marker: PhantomData,
        }
    }

    /// True when this copy reads the input's default: no cable, or no voice
    /// on a narrower cable. `is_connected` continues to report the cable itself.
    /// OwnPhase modules use this to run their own phase for padded voices.
    pub fn uses_default(&self, index: usize) -> bool {
        self.defaulted & (1 << index) != 0
    }

    /// Supplies the compiler's per-copy default mask.
    pub(crate) fn with_defaults(mut self, defaulted: u64) -> Self {
        self.defaulted = defaulted;
        self
    }

    /// Width of a bundle input. An unconnected input has one mono default voice.
    pub fn input_width(&self, port: usize) -> crate::Width {
        self.bundle_width(port, false)
    }

    /// Authoritative output capacity, including the source's structural voice count.
    pub fn output_width(&self, port: usize) -> crate::Width {
        self.bundle_width(port, true)
    }

    /// Reads one zero-based voice and side from a bundle input.
    ///
    /// Indices must be inside [`Self::input_width`]. Mono sides share one buffer.
    pub fn voice_input(&self, port: usize, voice: usize, side: usize) -> &'a [f32] {
        let width = self.input_width(port);
        assert!(voice < width.voices && side < 2, "bundle input index out of range");
        let ptr = self.bundles.get(port).map_or(self.inputs[port], |entry| {
            entry.channels[voice * width.sides + side.min(width.sides - 1)] as *const f32
        });
        // SAFETY: compiler and isolation callers provide readable bundle buffers
        // for offset + frames, with the same lifetime as every ordinary input.
        unsafe { std::slice::from_raw_parts(ptr.add(self.offset), self.frames) }
    }

    /// Writes one zero-based voice and side. Indices must be inside output width.
    pub fn voice_output(&mut self, port: usize, voice: usize, side: usize) -> &mut [f32] {
        let width = self.output_width(port);
        assert!(
            voice < width.voices && side < width.sides,
            "bundle output index out of range"
        );
        let ptr = self
            .bundles
            .get(self.inputs.len() + port)
            .map_or(self.outputs[port], |entry| entry.channels[voice * width.sides + side]);
        // SAFETY: outputs are distinct writable buffers. Borrowing self mutably
        // prevents any other output accessor from aliasing this returned slice.
        unsafe { std::slice::from_raw_parts_mut(ptr.add(self.offset), self.frames) }
    }

    /// Reads the compiler's per-port width without searching channel metadata.
    fn bundle_width(&self, port: usize, output: bool) -> crate::Width {
        let index = port + if output { self.inputs.len() } else { 0 };
        self.bundles.get(index).map_or(crate::Width::MONO, |entry| entry.width)
    }

    /// Number of frames to process in this call.
    #[inline]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Number of inputs.
    #[inline]
    pub fn input_count(&self) -> usize {
        self.inputs.len()
    }

    /// Number of outputs.
    #[inline]
    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    /// True when a cable is connected to input `index`.
    #[inline]
    pub fn is_connected(&self, index: usize) -> bool {
        index < 64 && self.connected & (1 << index) != 0
    }

    /// The per-sample values of a modulated parameter, one per frame of this
    /// call. `None` when the parameter has no assignment: use the value from
    /// [`Module::set_param`](crate::Module::set_param), which is constant for
    /// the block.
    ///
    /// Use each value for its frame only, in a local that falls back to the
    /// base. Do not store it in a field that `set_param` owns: the engine does
    /// not call `set_param` again when an assignment is removed, so the
    /// module would keep the last modulated value instead of the knob's.
    ///
    /// The slice does not borrow `self`, so it can be held across
    /// [`PortIo::split`].
    #[inline]
    pub fn modulated(&self, index: usize) -> Option<&'a [f32]> {
        let index = u16::try_from(index).ok()?;
        let ptr = self.params.iter().find(|(i, _)| *i == index)?.1;
        // SAFETY: guaranteed by the contract of `PortIo::new`.
        Some(unsafe { std::slice::from_raw_parts(ptr.add(self.offset), self.frames) })
    }

    /// Parameter `index` for this call, ready to read per frame with a
    /// fallback to the module's own value. See [`Param`].
    ///
    /// This is the usual way to read a parameter that can be modulated. It
    /// looks the parameter up once per call, and the result does not borrow
    /// `self`, so it can be held across [`PortIo::split`].
    #[inline]
    pub fn param(&self, index: usize) -> Param<'a> {
        Param {
            samples: self.modulated(index),
        }
    }

    /// The value of parameter `index` at `frame` within this call: the
    /// modulated value when the parameter has an assignment, otherwise
    /// `base`, the value the module keeps from
    /// [`Module::set_param`](crate::Module::set_param).
    ///
    /// It looks the parameter up on every call. In a loop over frames, call
    /// [`PortIo::param`] once before the loop instead. `frame` must be less
    /// than [`PortIo::frames`].
    #[inline]
    pub fn param_at_or(&self, index: usize, frame: usize, base: f32) -> f32 {
        self.param(index).at_or(frame, base)
    }

    /// Returns the first `I` inputs and the first `O` outputs at once.
    ///
    /// # Panics
    ///
    /// Panics if the module has fewer ports than requested. This is a
    /// programming error that tests catch, not a runtime condition.
    #[inline]
    pub fn split<const I: usize, const O: usize>(&mut self) -> ([&[f32]; I], [&mut [f32]; O]) {
        assert!(I <= self.inputs.len() && O <= self.outputs.len());
        let (offset, frames) = (self.offset, self.frames);
        let inputs = std::array::from_fn(|i| {
            // SAFETY: guaranteed by the contract of `PortIo::new`.
            unsafe { std::slice::from_raw_parts(self.inputs[i].add(offset), frames) }
        });
        let outputs = std::array::from_fn(|i| {
            // SAFETY: guaranteed by the contract of `PortIo::new`. Outputs are
            // distinct buffers, so the mutable slices do not overlap, and the
            // returned borrows are tied to `&mut self`.
            unsafe { std::slice::from_raw_parts_mut(self.outputs[i].add(offset), frames) }
        });
        (inputs, outputs)
    }

    /// One input buffer.
    #[inline]
    pub fn input(&self, index: usize) -> &[f32] {
        // SAFETY: guaranteed by the contract of `PortIo::new`.
        unsafe { std::slice::from_raw_parts(self.inputs[index].add(self.offset), self.frames) }
    }

    /// One output buffer.
    #[inline]
    pub fn output(&mut self, index: usize) -> &mut [f32] {
        // SAFETY: guaranteed by the contract of `PortIo::new`.
        unsafe { std::slice::from_raw_parts_mut(self.outputs[index].add(self.offset), self.frames) }
    }

    fn right_of(&self, index: usize, output: bool) -> Option<*mut f32> {
        let port = u16::try_from(index).ok()?;
        self.right
            .iter()
            .find(|right| right.port == port && right.output == output)
            .map(|right| right.ptr)
    }

    /// Both channels of a `Stereo` input, left then right. A mono cable or
    /// an unconnected input gives the same samples on both sides. On any
    /// other input this is the input twice.
    ///
    /// The slices do not borrow `self`, so they can be held while writing
    /// outputs.
    #[inline]
    pub fn stereo_input(&self, index: usize) -> [&'a [f32]; 2] {
        let (offset, frames) = (self.offset, self.frames);
        // SAFETY: guaranteed by the contract of `PortIo::new`. Inputs are
        // read-only for `'a` and never alias an output.
        let left = unsafe { std::slice::from_raw_parts(self.inputs[index].add(offset), frames) };
        let right = match self.right_of(index, false) {
            // SAFETY: as above.
            Some(ptr) => unsafe { std::slice::from_raw_parts(ptr.add(offset), frames) },
            None => left,
        };
        [left, right]
    }

    /// Both channels of a `Stereo` output, left then right.
    ///
    /// # Panics
    ///
    /// Panics if output `index` is not declared `Stereo`. This is a
    /// programming error that tests catch, not a runtime condition.
    #[inline]
    pub fn stereo_output(&mut self, index: usize) -> [&mut [f32]; 2] {
        let ([], [pair]) = self.split_outputs([], [index]);
        pair
    }

    /// Several outputs at once: the left buffer of each port in `mono`, and
    /// both channels of each port in `stereo`. Call it once above a frame loop.
    /// [`PortIo::output`] and [`PortIo::stereo_output`] each borrow `self`, so
    /// two of their results cannot be held together, and each call looks its
    /// buffers up again.
    ///
    /// ```ignore
    /// let ([gain], [[left, right]]) = io.split_outputs([Self::GAIN_OUT], [Self::OUT]);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if a port is named twice, or a port in `stereo` is not declared
    /// `Stereo`. This is a programming error that tests catch, not a runtime
    /// condition.
    #[inline]
    pub fn split_outputs<const M: usize, const S: usize>(
        &mut self,
        mono: [usize; M],
        stereo: [usize; S],
    ) -> ([&mut [f32]; M], [[&mut [f32]; 2]; S]) {
        // A real check, not a debug one: two slices of one port would overlap,
        // and that would be undefined behaviour.
        let ports = || mono.iter().chain(&stereo);
        for (n, port) in ports().enumerate() {
            assert!(
                !ports().skip(n + 1).any(|other| other == port),
                "split_outputs was given port {port} twice"
            );
        }
        let (offset, frames) = (self.offset, self.frames);
        let slice = |ptr: *mut f32| {
            // SAFETY: guaranteed by the contract of `PortIo::new`. The ports
            // are distinct (checked above), outputs are distinct buffers, and a
            // right buffer is distinct from every other buffer, so no slices
            // overlap. The returned borrows are tied to `&mut self`.
            unsafe { std::slice::from_raw_parts_mut(ptr.add(offset), frames) }
        };
        let mono = mono.map(|port| slice(self.outputs[port]));
        let stereo = stereo.map(|port| {
            let left = self.outputs[port];
            let right = self
                .right_of(port, true)
                .expect("a stereo output needs a port declared Channels::Stereo");
            // The compiler's `right_ptrs` promise this. A compiler bug should
            // fail a test here, not alias silently.
            debug_assert_ne!(left, right, "the two sides of output {port} are one buffer");
            [slice(left), slice(right)]
        });
        (mono, stereo)
    }
}

/// One parameter of a module for one call to [`Module::process`], from
/// [`PortIo::param`].
///
/// It answers "what is this parameter at frame `i`?" with a fallback: the
/// modulated value when the parameter has an assignment, otherwise the base
/// value the module passes in. The module keeps its base in the fields that
/// [`Module::set_param`] owns and never writes a modulated value there, so
/// when an assignment is removed the parameter returns to the knob's value.
/// `docs/learn/modulation-without-cables.md` shows what goes wrong otherwise.
///
/// It is `Copy` and does not borrow the [`PortIo`], so it can be held across
/// [`PortIo::split`]. Reading it is one branch and one indexed load, the same
/// cost as matching on [`PortIo::modulated`] by hand.
///
/// ```
/// use loom_core::{Module, PortIo, ProcessContext};
///
/// /// Adds a fixed offset to its input. Parameter 0 is the offset.
/// struct Offset {
///     offset: f32,
/// }
///
/// impl Module for Offset {
///     fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
///         let offset = io.param(0);
///         let ([input], [out]) = io.split::<1, 1>();
///         for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
///             *out = x + offset.at_or(i, self.offset);
///         }
///     }
///
///     fn set_param(&mut self, index: usize, value: f32) {
///         if index == 0 {
///             self.offset = value;
///         }
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Param<'a> {
    samples: Option<&'a [f32]>,
}

impl Param<'_> {
    /// The value at `frame`: the modulated value when the parameter has an
    /// assignment, otherwise `base`. `frame` must be less than
    /// [`PortIo::frames`].
    ///
    /// `base` is always evaluated, so a smoothed base keeps ticking while the
    /// parameter is modulated. Removing the assignment then continues the
    /// glide from where it is.
    #[inline]
    pub fn at_or(&self, frame: usize, base: f32) -> f32 {
        match self.samples {
            Some(samples) => samples[frame],
            None => base,
        }
    }

    /// Like [`Param::at_or`] for a parameter the module keeps in another
    /// form, such as a whole number, a switch, or a pitch. `convert` turns a
    /// modulated value into that form. It is not called when the parameter
    /// has no assignment, and then `base` is returned as it is.
    #[inline]
    pub fn map_or<T>(&self, frame: usize, base: T, convert: impl FnOnce(f32) -> T) -> T {
        match self.samples {
            Some(samples) => convert(samples[frame]),
            None => base,
        }
    }

    /// True when the parameter has an assignment in this call.
    #[inline]
    pub fn is_modulated(&self) -> bool {
        self.samples.is_some()
    }
}

/// Helpers for testing a module in isolation, without an engine.
pub mod testing {
    use super::*;

    /// Runs `module.process` once over the given buffers, with the transport
    /// stopped at position 0.
    ///
    /// Every input and output slice must have the same length.
    pub fn run(
        module: &mut dyn Module,
        sample_rate: f32,
        inputs: &[&[f32]],
        connected: &[bool],
        outputs: &mut [&mut [f32]],
        midi: &[MidiEvent],
    ) {
        run_with_transport(
            module,
            sample_rate,
            TransportInfo::default(),
            inputs,
            connected,
            outputs,
            midi,
        );
    }

    /// Like [`run`], with the given transport.
    pub fn run_with_transport(
        module: &mut dyn Module,
        sample_rate: f32,
        transport: TransportInfo,
        inputs: &[&[f32]],
        connected: &[bool],
        outputs: &mut [&mut [f32]],
        midi: &[MidiEvent],
    ) {
        run_with_transport_and_channel(module, sample_rate, transport, inputs, connected, outputs, midi, 0);
    }

    /// A test port layout. Primary channels come first, in descriptor order;
    /// extra voices and sides follow, grouped by port.
    #[derive(Debug, Clone)]
    pub struct BundleLayout {
        widths: Vec<crate::Width>,
    }

    impl BundleLayout {
        /// Validated widths for the ports in descriptor order.
        pub fn new(widths: Vec<crate::Width>) -> Self {
            assert!(
                widths
                    .iter()
                    .all(|width| (1..=16).contains(&width.voices) && (1..=2).contains(&width.sides))
            );
            Self { widths }
        }

        /// Number of flattened channel buffers.
        pub fn channels(&self) -> usize {
            self.widths.iter().map(|width| width.voices * width.sides).sum()
        }

        /// Number of ports.
        pub fn ports(&self) -> usize {
            self.widths.len()
        }

        /// Width of one port.
        pub fn width(&self, port: usize) -> crate::Width {
            self.widths[port]
        }

        /// Flattened buffer of a zero-based voice and side.
        pub fn index(&self, port: usize, voice: usize, side: usize) -> usize {
            let width = self.widths[port];
            assert!(voice < width.voices && side < width.sides);
            let channel = voice * width.sides + side;
            if channel == 0 {
                port
            } else {
                self.widths.len()
                    + self.widths[..port]
                        .iter()
                        .map(|width| width.voices * width.sides - 1)
                        .sum::<usize>()
                    + channel
                    - 1
            }
        }
    }

    /// Runs bundle-aware IO in isolation. All buffers have `ctx`'s frame count.
    /// This helper allocates its pointer tables before calling the module.
    #[allow(clippy::too_many_arguments)]
    pub fn run_bundles(
        module: &mut dyn Module,
        ctx: &ProcessContext<'_>,
        input_layout: &BundleLayout,
        inputs: &[&[f32]],
        connected: &[bool],
        output_layout: &BundleLayout,
        outputs: &mut [&mut [f32]],
    ) {
        assert_eq!(inputs.len(), input_layout.channels());
        assert_eq!(outputs.len(), output_layout.channels());
        assert_eq!(connected.len(), input_layout.ports());
        assert!(inputs.iter().all(|buffer| buffer.len() == ctx.frames));
        assert!(outputs.iter().all(|buffer| buffer.len() == ctx.frames));
        let inputs: Vec<_> = inputs.iter().map(|buffer| buffer.as_ptr()).collect();
        let outputs: Vec<_> = outputs.iter_mut().map(|buffer| buffer.as_mut_ptr()).collect();
        let mut bundles = Vec::new();
        let mut right = Vec::new();
        for (layout, output) in [(input_layout, false), (output_layout, true)] {
            for port in 0..layout.ports() {
                let width = layout.width(port);
                let mut channels = Vec::with_capacity(width.voices * width.sides);
                for voice in 0..width.voices {
                    for side in 0..width.sides {
                        let index = layout.index(port, voice, side);
                        let ptr = if output {
                            outputs[index]
                        } else {
                            inputs[index] as *mut f32
                        };
                        channels.push(ptr);
                        if voice == 0 && side == 1 {
                            right.push(RightPtr {
                                port: port as u16,
                                output,
                                ptr,
                            });
                        }
                    }
                }
                bundles.push(BundlePort {
                    width,
                    channels: channels.into_boxed_slice(),
                });
            }
        }
        let mask = connected
            .iter()
            .enumerate()
            .fold(0, |mask, (i, &connected)| mask | if connected { 1 << i } else { 0 });
        // SAFETY: buffer sizes are checked above. Shared inputs outlive the call;
        // mutable output slices prove that all outputs are distinct from each
        // other and the inputs. Metadata only names these same validated buffers.
        let mut io = unsafe {
            PortIo::new(
                &inputs[..input_layout.ports()],
                &outputs[..output_layout.ports()],
                &[],
                &right,
                &bundles,
                mask,
                0,
                ctx.frames,
            )
        };
        module.process(ctx, &mut io);
    }

    /// Like [`run_with_transport`], with the channel supplied to the module.
    #[allow(clippy::too_many_arguments)]
    pub fn run_with_transport_and_channel(
        module: &mut dyn Module,
        sample_rate: f32,
        transport: TransportInfo,
        inputs: &[&[f32]],
        connected: &[bool],
        outputs: &mut [&mut [f32]],
        midi: &[MidiEvent],
        channel: usize,
    ) {
        let frames = outputs
            .first()
            .map(|o| o.len())
            .or_else(|| inputs.first().map(|i| i.len()))
            .unwrap_or(0);
        assert!(inputs.iter().all(|i| i.len() == frames));
        assert!(outputs.iter().all(|o| o.len() == frames));
        let input_ptrs: Vec<*const f32> = inputs.iter().map(|i| i.as_ptr()).collect();
        let output_ptrs: Vec<*mut f32> = outputs.iter_mut().map(|o| o.as_mut_ptr()).collect();
        // The isolation harness exposes the left output to its caller. Give
        // each output a separate right buffer so stereo modules can run too.
        let mut right_buffers = vec![vec![0.0; frames]; outputs.len()];
        let right_ptrs: Vec<RightPtr> = right_buffers
            .iter_mut()
            .enumerate()
            .map(|(port, buffer)| RightPtr {
                port: u16::try_from(port).expect("port index fits in u16"),
                output: true,
                ptr: buffer.as_mut_ptr(),
            })
            .collect();
        let mask = connected
            .iter()
            .enumerate()
            .fold(0u64, |mask, (i, &c)| if c { mask | (1 << i) } else { mask });
        let ctx = ProcessContext::new(sample_rate, frames, midi)
            .with_transport(transport)
            .with_channel(channel);
        // SAFETY: the pointers come from distinct slices of length `frames`
        // that outlive this call; `&mut [&mut [f32]]` guarantees outputs are
        // unique and cannot alias the shared input slices. The separate
        // right buffers are also distinct and outlive the call.
        let mut io = unsafe { PortIo::new(&input_ptrs, &output_ptrs, &[], &right_ptrs, &[], mask, 0, frames) };
        module.process(&ctx, &mut io);
    }
}

#[cfg(test)]
mod tests;
