//! The app's timers and frames as deadlines (ui-layer.md, section 2,
//! rule 8).
//!
//! Each [`Timer`] runs while a condition on the app's state holds, as each
//! of Iced's `time::every` and `window::frames` subscriptions did. [`Timers`]
//! keeps the time each one is next due. After every update,
//! [`App::sync_timers`] starts the timers whose condition now holds, a
//! period from now, and stops the rest; the message a timer sends moves it
//! on by one period from when it was due (a fixed rate, as `time::every`
//! ticked). `App::next_deadline` gives the runtime the earliest,
//! and the bridge wakes the loop then.
//!
//! The frames that run work in `update` (`Frame` while measuring,
//! `LiveFrame` while modulation plays) are deadlines at display rate,
//! decided on 2026-11-14. Their period is the display's frame, as the
//! runtime gives it in `Context::window` (`WindowState::frame`): the
//! `winit` shell reads it from the monitor, and a driver that cannot keeps
//! 16 ms. [`App::update`] hands it to [`Timers`] before anything reads it.

use crate::{App, Message, a11y, devices, learn, recovery};
use std::time::{Duration, Instant};

/// Each check the app runs on a clock, and the message it sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Timer {
    /// A frame while `--measure` runs: the frame meter, live values,
    /// garbage, and dropouts.
    Frame,
    /// A frame while modulation plays, so live values follow it.
    LiveFrame,
    /// While recording: notices a write error, collects garbage, and ends
    /// finished crossfades.
    Tick,
    /// While a crash recovery copy may be due.
    Autosave,
    /// The MIDI input list, for a test host, which no thread reads.
    MidiPoll,
    /// While no device plays, or while audio input is wanted and missing.
    AudioPoll,
    /// While MIDI learn shows a ghost mark.
    MidiGhosts,
    /// While dropped files are being collected.
    DropsDone,
    /// While live values wait to be sent to a screen reader.
    LiveTick,
}

impl Timer {
    /// Every timer, in the order [`Timers`] keeps them.
    const ALL: [Self; 9] = [
        Self::Frame,
        Self::LiveFrame,
        Self::Tick,
        Self::Autosave,
        Self::MidiPoll,
        Self::AudioPoll,
        Self::MidiGhosts,
        Self::DropsDone,
        Self::LiveTick,
    ];

    /// How long from one message to the next, on a display whose frame
    /// lasts `frame`.
    pub(crate) fn period(self, frame: Duration) -> Duration {
        match self {
            Self::Frame | Self::LiveFrame => frame,
            Self::Tick => Duration::from_millis(500),
            Self::Autosave => recovery::TICK,
            Self::MidiPoll => devices::MIDI_POLL,
            Self::AudioPoll => Duration::from_millis(250),
            Self::MidiGhosts => learn::GHOST_CHECK,
            Self::DropsDone => Duration::from_millis(100),
            Self::LiveTick => a11y::LIVE_INTERVAL,
        }
    }

    /// The message it sends, due at `at`. A frame carries that time, the
    /// moment the frame was due, for the frame meter.
    pub(crate) fn message(self, at: Instant) -> Message {
        match self {
            Self::Frame => Message::Frame(at),
            Self::LiveFrame => Message::LiveFrame,
            Self::Tick => Message::Tick,
            Self::Autosave => Message::Autosave,
            Self::MidiPoll => Message::MidiPoll,
            Self::AudioPoll => Message::AudioPoll,
            Self::MidiGhosts => Message::MidiGhosts,
            Self::DropsDone => Message::DropsDone,
            Self::LiveTick => Message::A11y(a11y::Event::LiveTick),
        }
    }

    /// The timer that sends `message`, if one does.
    pub(crate) fn of(message: &Message) -> Option<Self> {
        Some(match message {
            Message::Frame(_) => Self::Frame,
            Message::LiveFrame => Self::LiveFrame,
            Message::Tick => Self::Tick,
            Message::Autosave => Self::Autosave,
            Message::MidiPoll => Self::MidiPoll,
            Message::AudioPoll => Self::AudioPoll,
            Message::MidiGhosts => Self::MidiGhosts,
            Message::DropsDone => Self::DropsDone,
            Message::A11y(a11y::Event::LiveTick) => Self::LiveTick,
            _ => return None,
        })
    }

    /// Its place in [`Timers`].
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|timer| *timer == self)
            .expect("every timer is listed")
    }
}

/// When each timer is next due, or `None` while it does not run, and how
/// long the display's frame lasts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Timers {
    /// When each timer is due, in the order of [`Timer::ALL`].
    due: [Option<Instant>; Timer::ALL.len()],
    /// The display's frame period, for the frames.
    frame: Duration,
}

impl Default for Timers {
    /// No timer runs, on a display whose rate is not known.
    fn default() -> Self {
        Self {
            due: [None; Timer::ALL.len()],
            frame: loom_ui::WindowState::FRAME,
        }
    }
}

impl Timers {
    /// Frames from now on last `frame`. A frame already due keeps its time,
    /// and the next one follows the new period.
    pub(crate) fn set_frame(&mut self, frame: Duration) {
        self.frame = frame;
    }

    /// Runs `timer` while `wanted`: a timer that starts is due a period
    /// after `now`, one that runs keeps its time, and one not wanted stops.
    pub(crate) fn keep(&mut self, timer: Timer, wanted: bool, now: Instant) {
        let period = timer.period(self.frame);
        let due = &mut self.due[timer.index()];
        *due = match (wanted, *due) {
            (false, _) => None,
            (true, Some(at)) => Some(at),
            (true, None) => Some(now + period),
        };
    }

    /// `timer`'s message was taken at `now`: it is due again a period
    /// after the time it was due, so the time a turn takes does not add up
    /// from one to the next, or at once when that has passed already. So
    /// frames run at display rate while they draw faster than that, and
    /// one after another while they draw slower, as the display's own
    /// frames did. The next `keep` stops it if it is no longer wanted.
    pub(crate) fn taken(&mut self, timer: Timer, now: Instant) {
        let period = timer.period(self.frame);
        let due = &mut self.due[timer.index()];
        *due = due.map(|at| (at + period).max(now));
    }

    /// When `timer` is next due.
    pub(crate) fn due(&self, timer: Timer) -> Option<Instant> {
        self.due[timer.index()]
    }

    /// The timer due first, and when. Two due at once go in the order of
    /// [`Timer::ALL`], and the second is due at once on the next turn.
    pub(crate) fn next(&self) -> Option<(Instant, Timer)> {
        Timer::ALL
            .iter()
            .filter_map(|&timer| Some((self.due(timer)?, timer)))
            .min_by_key(|(at, _)| *at)
    }
}

impl App {
    /// Starts and stops each timer by what the app's state says now, as
    /// the conditions on Iced's timer subscriptions did. Nothing runs before
    /// the window opens ([`Message::StartWatching`]), so no timer sends a
    /// test a message it did not ask for.
    pub(crate) fn sync_timers(&mut self, now: Instant) {
        if !self.watching {
            self.timers = Timers {
                frame: self.timers.frame,
                ..Timers::default()
            };
            return;
        }
        let stream = &self.session.engine.stream;
        let modulating = stream.is_playing() && self.session.controller.graph().assignments().any(|(_, a)| a.enabled);
        // While audio plays, the audio watcher reports stalled callbacks.
        // Once a device is lost, reconnection is tried by polling four
        // times a second (device-changes.md, Audio device loss).
        let waiting_for_input = self.rig.input.port().is_none() && stream.is_playing() && self.input_wanted();
        let audio_poll = !stream.without_device() && (!stream.is_playing() || waiting_for_input);
        // MIDI inputs plugged in and out are found by polling four times a
        // second: the systems offer no portable callback (device-changes.md).
        // A real host's list is read by a watcher thread (`watching.rs`).
        let midi_poll = self.rig.midi_in.is_some() && !self.rig.flags.no_midi && self.rig.host.midi_lister().is_none();
        let wanted = [
            // Redraw every frame, so the counter measures real render cost.
            (Timer::Frame, self.measuring),
            // Live modulated values move every block. Without audio the
            // engine does not run, so nothing moves.
            (Timer::LiveFrame, !self.measuring && modulating),
            // Otherwise the window redraws only when something changes
            // (#90). A recording ticks to notice a write error, and a slot
            // swap ticks until its fade is done, so the leaving part is
            // removed (sub-patches.md, How a swap crossfades). With no
            // audio device the engine is idle and the fade does not end,
            // so it ticks at that slow rate until a device starts.
            (
                Timer::Tick,
                !self.measuring && (self.rig.recorder.is_some() || self.session.hierarchy.is_fading()),
            ),
            // Only while a copy may be due: once it matches the patch, an
            // idle app ticks no more (#90).
            (Timer::Autosave, self.run_files.autosave_due(&self.session.document)),
            (Timer::MidiPoll, midi_poll),
            (Timer::AudioPoll, audio_poll),
            // The rest run only while there is something to finish, so an
            // idle app stays idle.
            (Timer::MidiGhosts, !self.session.learn.ghosts().shown().is_empty()),
            (Timer::DropsDone, !self.drops.is_empty()),
            (Timer::LiveTick, self.a11y.live_waits()),
        ];
        for (timer, wanted) in wanted {
            self.timers.keep(timer, wanted, now);
        }
    }

    /// The next moment the app needs to wake: the crash recovery question
    /// (`dialogs.rs`) or the first timer due, whichever comes first. A tie
    /// goes to the question, which is due at once.
    pub(crate) fn next_deadline(&self) -> Option<loom_ui::Deadline<Message>> {
        let timer = self.timers.next().map(|(at, timer)| loom_ui::Deadline {
            at,
            message: timer.message(at),
        });
        match (self.recovery_deadline(), timer) {
            (Some(question), Some(timer)) if timer.at < question.at => Some(timer),
            (Some(question), _) => Some(question),
            (None, timer) => timer,
        }
    }
}

#[cfg(test)]
mod tests;
