//! The module API (FR-SDK-01).
//!
//! Built-in and third-party modules implement the same trait. A module only
//! processes samples. Its ports, parameters, and metadata live in a static
//! [`ModuleDescriptor`](crate::ModuleDescriptor).

use crate::midi::MidiEvent;
use std::marker::PhantomData;

/// A DSP module instance.
///
/// Rules for `process` (NFR-RT-01): no allocation, no locks, no system calls,
/// no blocking. The engine may call `process` with any number of frames from
/// 1 to the configured maximum block size. Inside feedback loops it is called
/// one frame at a time (ADR-0002), so results must not depend on how frames
/// are grouped into calls.
pub trait Module: Send {
    /// Processes `io.frames()` frames.
    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>);

    /// Sets a parameter by index. Values are already clamped to the range in
    /// the descriptor. Called on the audio thread, so it must be cheap.
    fn set_param(&mut self, index: usize, value: f32) {
        let _ = (index, value);
    }

    /// Clears internal state (envelopes, filter memory, oscillator phase).
    fn reset(&mut self) {}
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
}

impl<'a> ProcessContext<'a> {
    /// Creates a context. Mainly useful for testing modules in isolation.
    pub fn new(sample_rate: f32, frames: usize, midi: &'a [MidiEvent]) -> Self {
        Self {
            sample_rate,
            midi,
            midi_base: 0,
            frames,
        }
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

/// Access to a module's input and output buffers for one call.
///
/// Inputs are read-only. Unconnected inputs read zeros. Use
/// [`PortIo::is_connected`] to choose a fallback (for example a parameter
/// value) when an input has no cable.
pub struct PortIo<'a> {
    inputs: &'a [*const f32],
    outputs: &'a [*mut f32],
    connected: u64,
    offset: usize,
    frames: usize,
    _marker: PhantomData<&'a mut f32>,
}

impl<'a> PortIo<'a> {
    /// # Safety
    ///
    /// Every pointer must be valid for reads (inputs) or writes (outputs) of
    /// `offset + frames` floats for `'a`. No output may alias any input or
    /// any other output.
    pub(crate) unsafe fn new(
        inputs: &'a [*const f32],
        outputs: &'a [*mut f32],
        connected: u64,
        offset: usize,
        frames: usize,
    ) -> Self {
        Self {
            inputs,
            outputs,
            connected,
            offset,
            frames,
            _marker: PhantomData,
        }
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
}

/// Helpers for testing a module in isolation, without an engine.
pub mod testing {
    use super::*;

    /// Runs `module.process` once over the given buffers.
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
        let frames = outputs
            .first()
            .map(|o| o.len())
            .or_else(|| inputs.first().map(|i| i.len()))
            .unwrap_or(0);
        assert!(inputs.iter().all(|i| i.len() == frames));
        assert!(outputs.iter().all(|o| o.len() == frames));
        let input_ptrs: Vec<*const f32> = inputs.iter().map(|i| i.as_ptr()).collect();
        let output_ptrs: Vec<*mut f32> = outputs.iter_mut().map(|o| o.as_mut_ptr()).collect();
        let mask = connected
            .iter()
            .enumerate()
            .fold(0u64, |mask, (i, &c)| if c { mask | (1 << i) } else { mask });
        let ctx = ProcessContext::new(sample_rate, frames, midi);
        // SAFETY: the pointers come from distinct slices of length `frames`
        // that outlive this call; `&mut [&mut [f32]]` guarantees outputs are
        // unique and cannot alias the shared input slices.
        let mut io = unsafe { PortIo::new(&input_ptrs, &output_ptrs, mask, 0, frames) };
        module.process(&ctx, &mut io);
    }
}
