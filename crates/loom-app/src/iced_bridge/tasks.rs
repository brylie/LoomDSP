//! Running Iced tasks without Iced's runtime.
//!
//! The bridge still hands back Iced tasks: the widget operations for focus
//! and scrolling, the window questions, and the focus query. A task is a
//! stream of Iced actions. [`run`] polls one to its end and carries out the
//! actions it can, through a [`Sink`]: a widget operation on the view, an
//! output back to the caller, and the clipboard. The `Ui` harness runs its
//! tasks here today, and the window's layers will at plan step 2.2b-ii, so
//! both carry tasks out the same way.

use iced::Task;
use iced::advanced::widget::Operation;
use iced_runtime::Action;
use iced_runtime::futures::futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

/// How many times in a row a stream may wake itself before one action.
/// An Iced chain yields once per step, so this is far above any real
/// chain, and it stops a future that never settles from spinning forever.
const SELF_WAKES: usize = 4096;

/// A waker that only records that it was woken.
#[derive(Default)]
struct Woken(AtomicBool);

impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Where a task's actions go.
pub(crate) trait Sink {
    /// Runs a widget operation on the view, and those it chains.
    fn operate(&mut self, operation: Box<dyn Operation>);
    /// The clipboard's text.
    fn clipboard_read(&mut self) -> Option<String>;
    /// Puts `contents` on the clipboard.
    fn clipboard_write(&mut self, contents: String);
}

/// Why a task stopped before its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stuck {
    /// It waits on something outside, such as a timer or a dialog.
    Waits,
    /// It woke itself [`SELF_WAKES`] times in a row and never went on, so
    /// it would spin forever inside one turn.
    Spins,
    /// It asked for an action no [`Sink`] carries out, named here.
    Action(String),
}

/// What came of running one task.
#[derive(Debug)]
pub(crate) struct Ran<T> {
    /// Its outputs, in order.
    pub outputs: Vec<T>,
    /// Why it stopped early, if it did. Outputs before that are kept.
    pub stuck: Option<Stuck>,
}

/// Runs `task` to its end, carrying its actions out through `sink`.
pub(crate) fn run<T: std::fmt::Debug>(task: Task<T>, sink: &mut impl Sink) -> Ran<T> {
    let mut outputs = Vec::new();
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return Ran { outputs, stuck: None };
    };
    let woken = Arc::new(Woken::default());
    let waker = Waker::from(woken.clone());
    let mut context = Context::from_waker(&waker);
    loop {
        // A stream returns Pending while its parts hand over, for example
        // between the steps of a chain or after a clipboard reply. It then
        // wakes itself before returning, so it can go on at once. A stream
        // that returns Pending without waking itself waits on the outside
        // world.
        let mut wakes = 0;
        let action = loop {
            woken.0.store(false, Ordering::Relaxed);
            let stuck = match stream.poll_next_unpin(&mut context) {
                Poll::Ready(action) => break action,
                Poll::Pending if !woken.0.load(Ordering::Relaxed) => Stuck::Waits,
                Poll::Pending if wakes == SELF_WAKES => Stuck::Spins,
                Poll::Pending => {
                    wakes += 1;
                    continue;
                }
            };
            return Ran {
                outputs,
                stuck: Some(stuck),
            };
        };
        let Some(action) = action else {
            return Ran { outputs, stuck: None };
        };
        match action {
            Action::Output(output) => outputs.push(output),
            Action::Widget(operation) => sink.operate(operation),
            Action::Clipboard(iced_runtime::clipboard::Action::Read { channel, .. }) => {
                let _ = channel.send(sink.clipboard_read());
            }
            Action::Clipboard(iced_runtime::clipboard::Action::Write { contents, .. }) => {
                sink.clipboard_write(contents);
            }
            other => {
                return Ran {
                    outputs,
                    stuck: Some(Stuck::Action(format!("{other:?}"))),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests;
