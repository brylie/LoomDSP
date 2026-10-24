//! An output that plays only when a test asks (`docs/design/audio-qa.md`,
//! Part 1).
//!
//! The clock output runs on a timer, so a test that checks what was played
//! can only sleep and hope. A stepped output instead plays the engine when
//! the test calls [`Tape::step`], and keeps every sample it plays. Time is
//! counted in frames, so the same test gives the same tape on any machine.

use crate::Render;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Everything stepped outputs played, across every stream that shared the
/// tape, in order. Clones share the same tape.
#[derive(Clone, Default)]
pub struct Tape(Arc<Mutex<State>>);

/// The tape's contents and the stream playing now.
#[derive(Default)]
struct State {
    /// The stream playing now, if any.
    render: Option<Render>,
    /// Frames per block of the stream playing now.
    block: usize,
    /// Interleaved stereo scratch, one block long.
    buffer: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    /// The frame index where each stream started.
    starts: Vec<usize>,
}

impl Tape {
    /// An empty tape.
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // Only tests and the control thread use a tape; a panic in one of
        // them leaves the samples readable.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Plays the stream playing now for at least `frames` frames, in whole
    /// blocks of its buffer size, as a device would. With no stream
    /// playing, nothing happens: a stopped device plays nothing.
    pub fn step(&self, frames: usize) {
        let mut played = 0;
        while played < frames {
            let n = self.play_block();
            if n == 0 {
                return;
            }
            played += n;
        }
    }

    /// Plays one block of the stream playing now. Returns the frames
    /// played: 0 when no stream plays.
    fn play_block(&self) -> usize {
        let mut guard = self.state();
        let State {
            render,
            block,
            buffer,
            left,
            right,
            ..
        } = &mut *guard;
        let Some(render) = render else { return 0 };
        render.render(buffer);
        for [l, r] in buffer.as_chunks::<2>().0 {
            left.push(*l);
            right.push(*r);
        }
        *block
    }

    /// The left channel, from the first stream to now.
    pub fn left(&self) -> Vec<f32> {
        self.state().left.clone()
    }

    /// The right channel, from the first stream to now.
    pub fn right(&self) -> Vec<f32> {
        self.state().right.clone()
    }

    /// Frames on the tape.
    pub fn len(&self) -> usize {
        self.state().left.len()
    }

    /// True when nothing has been played.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The frame index where each stream started, in order.
    pub fn stream_starts(&self) -> Vec<usize> {
        self.state().starts.clone()
    }

    /// True while a stream is attached and would play on the next step.
    pub fn is_playing(&self) -> bool {
        self.state().render.is_some()
    }

    /// Makes `render` the stream playing now, and marks where it starts.
    pub(crate) fn attach(&self, render: Render, block: usize) -> Attached {
        let mut state = self.state();
        let start = state.left.len();
        state.starts.push(start);
        state.block = block.max(1);
        state.buffer = vec![0.0; state.block * 2];
        state.render = Some(render);
        drop(state);
        Attached { tape: self.clone() }
    }
}

/// A stream playing on a tape. Dropping it stops the stream.
pub(crate) struct Attached {
    tape: Tape,
}

impl Attached {
    /// Frames per block of this stream.
    pub(crate) fn block(&self) -> usize {
        self.tape.state().block
    }

    /// Plays blocks until `done` says so, or until `limit` frames have been
    /// played. Returns true if it stopped because `done` said so.
    pub(crate) fn play_until(&self, limit: usize, done: impl Fn() -> bool) -> bool {
        let mut played = 0;
        loop {
            if done() {
                return true;
            }
            if played >= limit {
                return false;
            }
            let n = self.tape.play_block();
            if n == 0 {
                return false;
            }
            played += n;
        }
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.tape.state().render = None;
    }
}
