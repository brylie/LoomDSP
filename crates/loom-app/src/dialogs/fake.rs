//! The test driver for direct calls: [`App::send`] runs one message
//! through `update` as the runtime would, with dialogs answered from the
//! fakes and background work run inline (ui-layer.md, section 2, rules 3
//! and 4).

use super::*;
use crate::files::fake as files_fake;
use loom_ui::effect::FileAction;
use loom_ui::{Context, Effect, Waker, WindowState};
use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::{Arc, mpsc};

thread_local! {
    static QUESTION_ANSWER: Cell<Choice> = const { Cell::new(None) };
}

/// Native questions other than the credit question are answered with
/// `choice` from now on, on this thread. Without a call they are closed
/// with no button.
pub(crate) fn answer_question(choice: Choice) {
    QUESTION_ANSWER.with(|answer| answer.set(choice));
}

/// The fake answer to a file dialog: from `files::fake`.
pub(crate) fn file_answer(request: &loom_ui::effect::FileRequest) -> Option<std::path::PathBuf> {
    match request.action {
        FileAction::Open => files_fake::open_answer(),
        FileAction::Save => files_fake::save_answer(),
    }
}

/// The fake answer to a native question: the credit question, known by its
/// first button, from `files::fake`, and any other from [`answer_question`].
pub(crate) fn question_answer(question: &loom_ui::effect::Question) -> Choice {
    if question.buttons.first().map(String::as_str) == Some(crate::files::ADD_MY_NAME) {
        files_fake::add_author_choice()
    } else {
        QUESTION_ANSWER.with(Cell::get)
    }
}

/// Answers a dialog `effect` from the fakes, sending its reply's message
/// through `waker` ([`file_answer`], [`question_answer`]). Gives back any
/// effect that is not a dialog.
pub(crate) fn answer(effect: Effect<Message>, waker: &Waker<Message>) -> Option<Effect<Message>> {
    match effect {
        Effect::FileDialog { request, reply } => {
            waker.send(reply.give(file_answer(&request)));
            None
        }
        Effect::Ask { question, reply } => {
            waker.send(reply.give(question_answer(&question)));
            None
        }
        other => Some(other),
    }
}

impl App {
    /// Applies `message` as one turn of the runtime would, then the turns
    /// its answers make, until none is left.
    ///
    /// - A file dialog is answered from `files::fake`, and the credit
    ///   question too; any other question from [`answer_question`].
    /// - Work runs inline, to its end, and its messages follow in order.
    /// - Every other effect is returned, unrun, for the test to read.
    /// - An input event is routed first, as the bridge routes it, and each
    ///   message it becomes is applied in turn.
    /// - Window questions are dropped: there is no window to ask
    ///   (`window_question.rs`). The `Ui` harness answers them.
    pub(crate) fn send(&mut self, message: Message) -> Effects<Message> {
        let (sender, queue) = mpsc::channel();
        let waker = Waker::new(sender, Arc::new(|| {}));
        let window = WindowState::new(loom_ui_core::Size::new(crate::WINDOW.width, crate::WINDOW.height));
        let mut unrun = Effects::none();
        let mut pending = VecDeque::from(crate::input_owner::route(message));
        while let Some(message) = pending.pop_front() {
            // No window here: the app's own copy stands in for the
            // runtime's record of focus, as `FocusMoved` keeps it.
            let cx = Context::new(Instant::now(), &waker, self.focus.clone(), window);
            let effects = self.update(message, &cx);
            drop(self.take_window_questions());
            for effect in effects {
                match answer(effect, &waker) {
                    Some(Effect::Work(job)) => job.run(&waker),
                    // As the runtime does: after what was sent so far.
                    Some(Effect::NextTurn(messages)) => {
                        pending.extend(queue.try_iter().flat_map(crate::input_owner::route));
                        pending.extend(messages);
                    }
                    Some(other) => unrun.push(other),
                    None => {}
                }
            }
            pending.extend(queue.try_iter().flat_map(crate::input_owner::route));
        }
        unrun
    }

    /// Applies the messages posted outside an update, as the next turn
    /// would: for a test that calls a handler such as `a11y_act` directly.
    pub(crate) fn run_posted(&mut self) {
        for message in std::mem::take(&mut self.posted) {
            self.send(message);
        }
    }

    /// The messages posted outside an update, for the `Ui` harness to
    /// deliver as the next turn would.
    pub(crate) fn take_posted(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.posted)
    }

    /// Answers the crash recovery question with `prompt`, through the turns
    /// the app takes: the question as an effect, then its answer.
    pub(crate) fn answer_recovery_with(&mut self, prompt: &mut dyn recovery::Prompt) {
        let Some(offer) = &self.recovery_offer else {
            return;
        };
        let button = match prompt.ask(&offer.orphan) {
            recovery::Choice::Restore => Some(0),
            recovery::Choice::Discard => Some(1),
            recovery::Choice::NotNow => Some(2),
        };
        answer_question(button);
        self.send(Message::AskRecovery);
    }
}
