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

/// Answers a dialog `effect` from the fakes, sending its reply's message
/// through `waker`: a file dialog from `files::fake`, the credit question
/// too, and any other question from [`answer_question`]. Gives back any
/// effect that is not a dialog.
pub(crate) fn answer(app: &App, effect: Effect<Message>, waker: &Waker<Message>) -> Option<Effect<Message>> {
    match effect {
        Effect::FileDialog { request, reply } => {
            let path = match request.action {
                FileAction::Open => files_fake::open_answer(),
                FileAction::Save => files_fake::save_answer(),
            };
            waker.send(reply.give(path));
            None
        }
        Effect::Ask { reply, .. } => {
            let choice = match app.waiting() {
                Some(Waiting::Credit { .. }) => files_fake::add_author_choice(),
                _ => QUESTION_ANSWER.with(Cell::get),
            };
            waker.send(reply.give(choice));
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
    /// - Iced tasks from code not yet ported are dropped, as the one that
    ///   `update` returned was dropped before the port.
    pub(crate) fn send(&mut self, message: Message) -> Effects<Message> {
        let (sender, queue) = mpsc::channel();
        let waker = Waker::new(sender, Arc::new(|| {}));
        let window = WindowState::new(loom_ui_core::Size::new(crate::WINDOW.width, crate::WINDOW.height));
        let mut unrun = Effects::none();
        let mut pending = VecDeque::from([message]);
        while let Some(message) = pending.pop_front() {
            // No window here: the app's own copy stands in for the
            // runtime's record of focus, as `FocusMoved` keeps it.
            let cx = Context::new(Instant::now(), &waker, self.focus.clone(), window);
            let effects = self.update(message, &cx);
            drop(self.outbox.take());
            for effect in effects {
                match answer(self, effect, &waker) {
                    Some(Effect::Work(job)) => job.run(&waker),
                    Some(other) => unrun.push(other),
                    None => {}
                }
            }
            pending.extend(queue.try_iter());
        }
        unrun
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
