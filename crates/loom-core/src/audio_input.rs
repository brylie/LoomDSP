//! Lock-free audio input transport (audio-input.md).
//!
//! The producer writes interleaved frames from the input callback. The
//! consumer prepares one block for all Audio In modules, so they read the
//! same frames without competing for the ring.

use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Maximum number of input channels supported by the patch.
pub const MAX_INPUT_CHANNELS: usize = 8;

/// One input frame, with unused channels set to zero.
pub type InputFrame = [f32; MAX_INPUT_CHANNELS];

/// Writes input frames from one input callback thread.
pub struct InputWriter {
    tx: Producer<InputFrame>,
    status: InputStatus,
}

/// Shared stream health. A callback reports failure without locking.
#[derive(Clone)]
pub struct InputStatus(Arc<InputHealth>);

/// Callback health shared without locks.
struct InputHealth {
    live: AtomicBool,
    callbacks: AtomicU64,
}

impl InputStatus {
    /// Marks the stream unavailable; the engine then outputs silence.
    pub fn fail(&self) {
        self.0.live.store(false, Ordering::Release);
    }

    /// True while the input stream is available.
    pub fn is_live(&self) -> bool {
        self.0.live.load(Ordering::Acquire)
    }

    /// Number of input callbacks received.
    pub fn callbacks(&self) -> u64 {
        self.0.callbacks.load(Ordering::Relaxed)
    }

    /// Identity of this stream, for restarting watchers when it changes.
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

/// Reads input frames on the audio thread without allocation or waiting.
pub struct InputReader {
    rx: Consumer<InputFrame>,
    status: InputStatus,
    target: usize,
    channels: usize,
    waiting: bool,
    underruns: u64,
    overruns: u64,
}

/// Allocates an input ring holding four times `buffer_frames` on the control
/// thread. `channels` is the number the device supplies, from one to eight.
///
/// # Panics
///
/// Panics if the buffer size is zero, overflows, or the channel count is
/// outside the supported range.
pub fn input_ring(buffer_frames: usize, channels: usize) -> (InputWriter, InputReader) {
    assert!(buffer_frames > 0 && (1..=MAX_INPUT_CHANNELS).contains(&channels));
    let capacity = buffer_frames.checked_mul(4).expect("input ring size fits");
    let (tx, rx) = RingBuffer::new(capacity);
    let status = InputStatus(Arc::new(InputHealth {
        live: AtomicBool::new(true),
        callbacks: AtomicU64::new(0),
    }));
    (
        InputWriter {
            tx,
            status: status.clone(),
        },
        InputReader {
            rx,
            status,
            target: buffer_frames,
            channels,
            waiting: true,
            underruns: 0,
            overruns: 0,
        },
    )
}

impl InputWriter {
    /// A handle for reporting a stream failure from the error callback.
    pub fn status(&self) -> InputStatus {
        self.status.clone()
    }
    /// Marks one device callback, so a watcher can detect a stalled stream.
    pub fn callback_received(&self) {
        self.status.0.callbacks.fetch_add(1, Ordering::Relaxed);
    }
    /// Queues one frame. Returns false when the ring is full.
    pub fn push(&mut self, frame: InputFrame) -> bool {
        self.tx.push(frame).is_ok()
    }
}

impl Drop for InputWriter {
    fn drop(&mut self) {
        self.status.fail();
    }
}

impl InputReader {
    /// Number of channels supplied by the device.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Number of underrun corrections since the ring opened.
    pub fn underruns(&self) -> u64 {
        self.underruns
    }

    /// Number of overrun corrections since the ring opened.
    pub fn overruns(&self) -> u64 {
        self.overruns
    }

    /// Fills one engine block. An underrun produces silence for the whole
    /// block and pauses until the target fill is available. An overrun
    /// discards the oldest frames down to the target before reading.
    pub fn read_block(&mut self, output: &mut [InputFrame]) -> bool {
        output.fill([0.0; MAX_INPUT_CHANNELS]);
        if !self.status.is_live() {
            return false;
        }
        let available = self.rx.slots();
        if self.waiting {
            if available < self.target.max(output.len()) {
                return false;
            }
            self.waiting = false;
        }
        if available < output.len() {
            self.waiting = true;
            self.underruns += 1;
            return false;
        }
        if available > self.target.saturating_mul(2) {
            for _ in 0..available - self.target.max(output.len()) {
                let _ = self.rx.pop();
            }
            self.overruns += 1;
        }
        for frame in output {
            *frame = self.rx.pop().expect("available frames checked above");
        }
        true
    }
}

#[cfg(test)]
mod tests;
