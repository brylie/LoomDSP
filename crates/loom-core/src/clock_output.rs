//! Bounded MIDI clock egress. The host maps sample frames to playback time.

use crate::TransportInfo;
use core::time::Duration;
use loom_midi::{ClockMessage, PULSES_PER_QUARTER};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One MIDI clock message and its intended playback frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockEvent {
    /// Absolute engine frame.
    pub frame: u64,
    /// The message to send.
    pub message: ClockMessage,
    /// Transport position at this event, in quarter notes.
    pub position: f64,
    /// Duration of one pulse interval at the current tempo.
    pub pulse_period: Duration,
}

/// The host end of the bounded MIDI clock queue.
pub struct ClockReader {
    events: Consumer<ClockEvent>,
    shared: Arc<Shared>,
}

impl ClockReader {
    /// Enables generation while the host has an endpoint to receive clock.
    pub fn set_enabled(&self, enabled: bool) {
        self.shared.enabled.store(enabled, Ordering::Release);
    }

    /// Retrieves the next message, if any.
    pub fn pop(&mut self) -> Option<ClockEvent> {
        self.events.pop().ok()
    }

    /// Number of messages that could not be queued.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Acquire)
    }

    /// Latest position and play state for a newly opened or recovering output.
    pub fn state(&self) -> (f64, bool) {
        let encoded = f64::from_bits(self.shared.state.load(Ordering::Acquire));
        (encoded.abs(), encoded.is_sign_positive())
    }
}

/// The latest state and queue overflow count.
#[derive(Default)]
struct Shared {
    enabled: AtomicBool,
    state: AtomicU64,
    dropped: AtomicU64,
}

/// The producer used by the audio thread.
pub(crate) struct ClockOutput {
    events: Producer<ClockEvent>,
    shared: Arc<Shared>,
    playing: bool,
    next_pulse: u64,
}

/// Creates the fixed-capacity queue before audio starts.
pub(crate) fn channel() -> (ClockOutput, ClockReader) {
    let (send, receive) = RingBuffer::new(256);
    let shared = Arc::new(Shared::default());
    shared.enabled.store(true, Ordering::Relaxed);
    shared.state.store((-0.0_f64).to_bits(), Ordering::Relaxed);
    (
        ClockOutput {
            events: send,
            shared: shared.clone(),
            playing: false,
            next_pulse: 0,
        },
        ClockReader {
            events: receive,
            shared,
        },
    )
}

impl ClockOutput {
    /// Queues a transport transition at the frame that changes its state.
    pub fn transition(&mut self, frame: u64, transport: TransportInfo) -> bool {
        let position = transport.position();
        let playing = transport.playing();
        let located = transport.jumped();
        self.shared.state.store(
            position.copysign(if playing { 1.0 } else { -1.0 }).to_bits(),
            Ordering::Release,
        );
        if !self.shared.enabled.load(Ordering::Acquire) {
            self.playing = false;
            return false;
        }
        let pulse_period = Duration::from_secs_f64(60.0 / transport.tempo() / f64::from(PULSES_PER_QUARTER));
        if !playing && self.playing {
            self.send(frame, ClockMessage::Stop, position, pulse_period);
        }
        if located || (playing && !self.playing && position != 0.0) {
            self.send(frame, ClockMessage::at_position(position), position, pulse_period);
        }
        if playing && !self.playing {
            self.send(
                frame,
                if position == 0.0 {
                    ClockMessage::Start
                } else {
                    ClockMessage::Continue
                },
                position,
                pulse_period,
            );
        }
        if located || (playing && !self.playing) {
            self.next_pulse = (position * f64::from(PULSES_PER_QUARTER)).ceil() as u64;
        }
        self.playing = playing;
        playing
    }

    /// Generates pulses within this block.
    pub fn render(&mut self, frame: u64, frames: usize, transport: TransportInfo) {
        if !self.transition(frame, transport) {
            return;
        }
        let pulse_period = Duration::from_secs_f64(60.0 / transport.tempo() / f64::from(PULSES_PER_QUARTER));
        let end = transport.position_at(frames - 1);
        if !end.is_finite() {
            return;
        }
        let mut start = 0;
        loop {
            let beat = self.next_pulse as f64 / f64::from(PULSES_PER_QUARTER);
            if end < beat {
                break;
            }
            let mut lo = start;
            let mut hi = frames - 1;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if transport.position_at(mid) < beat {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            self.send(
                frame + lo as u64,
                ClockMessage::Pulse,
                transport.position_at(lo),
                pulse_period,
            );
            self.next_pulse += 1;
            start = lo;
        }
    }

    /// Queues a message or records overflow for the host to recover.
    fn send(&mut self, frame: u64, message: ClockMessage, position: f64, pulse_period: Duration) {
        if self
            .events
            .push(ClockEvent {
                frame,
                message,
                position,
                pulse_period,
            })
            .is_err()
        {
            self.shared.dropped.fetch_add(1, Ordering::Release);
        }
    }
}
