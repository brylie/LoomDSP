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
use iced_runtime::futures::futures::{FutureExt, StreamExt};

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
    loop {
        // A stream can return Pending once while its parts hand over, for
        // example after a clipboard reply. A few polls tell that apart from
        // a task that waits on the outside world.
        let polled = (0..8).find_map(|_| stream.next().now_or_never());
        let action = match polled {
            Some(Some(action)) => action,
            Some(None) => return Ran { outputs, stuck: None },
            None => {
                return Ran {
                    outputs,
                    stuck: Some(Stuck::Waits),
                };
            }
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
