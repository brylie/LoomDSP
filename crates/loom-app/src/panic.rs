//! Panic in the app (`docs/design/panic.md`): the key, the toolbar button,
//! and the MIDI trigger in Preferences. The engine does the work
//! ([`loom_core::Controller::panic`]). Panic is not an edit: it makes no
//! undo step and does not mark the patch changed.

use crate::preferences::PanicPref;
use crate::{App, Message};
use iced::keyboard::{self, Key};

/// Cmd+. on macOS and Ctrl+. elsewhere runs panic. It is read from every
/// key event, taken by a widget or not, so it works with a text field
/// focused, with a window open, and during a drag (panic.md, How to run
/// it). A held key does not repeat it.
pub fn key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key, modifiers, repeat, ..
    }) = event
    else {
        return None;
    };
    (is_panic_key(&key, modifiers) && !repeat).then_some(Message::Panic)
}

/// True for the panic shortcut: `.` with the platform's command key.
pub fn is_panic_key(key: &Key, modifiers: keyboard::Modifiers) -> bool {
    matches!(key.as_ref(), Key::Character(".")) && modifiers.command()
}

impl App {
    /// Runs panic: the engine fades out, releases every note, resets every
    /// module, and fades back in. The Hold note button lets go too, since
    /// its note is released.
    pub(crate) fn panic(&mut self) {
        match self.controller.panic() {
            Ok(()) => {
                self.holding = false;
                self.status = "Panic: every note released and every module reset.".to_string();
            }
            Err(e) => self.status = format!("Panic: {e}"),
        }
    }

    /// Sends the saved panic trigger to the engine. Called for every new
    /// engine, because the trigger belongs to the rig, not to one engine.
    pub(crate) fn send_panic_trigger(&mut self) {
        let trigger = self.rig.store.saved().panic.map(PanicPref::trigger);
        if let Err(e) = self.controller.set_panic_trigger(trigger) {
            self.status = format!("Panic: {e}");
        }
    }

    /// Starts or stops learning the panic trigger: the next control change
    /// or note on after this becomes the trigger.
    pub(crate) fn learn_panic(&mut self) {
        if self.rig.panic_learn.is_some() {
            self.rig.panic_learn = None;
            self.status = "Panic: learn cancelled.".to_string();
            return;
        }
        let cc = self.controller.last_control_change().map(|seen| seen.serial);
        let note = self.controller.last_note_on().map(|seen| seen.serial);
        self.rig.panic_learn = Some(cc.max(note).unwrap_or(0));
        self.status = "Panic: move a control or press a pad to set the trigger.".to_string();
    }

    /// Takes the first control change or note on that arrived since learn
    /// started as the panic trigger. Returns true if it set one.
    pub(crate) fn panic_midi_arrived(&mut self) -> bool {
        let Some(since) = self.rig.panic_learn else {
            return false;
        };
        let cc = self
            .controller
            .last_control_change()
            .filter(|seen| seen.serial > since)
            .map(|seen| {
                (
                    seen.serial,
                    PanicPref::Cc(seen.control.channel, seen.control.controller),
                )
            });
        let note = self
            .controller
            .last_note_on()
            .filter(|seen| seen.serial > since)
            .map(|seen| (seen.serial, PanicPref::Note(seen.channel, seen.note)));
        // Between two checks both can arrive: the later one is the one the
        // player meant last, but either is a fair pick. Take the later.
        let Some((_, learned)) = cc.into_iter().chain(note).max_by_key(|(serial, _)| *serial) else {
            return false;
        };
        self.rig.panic_learn = None;
        self.set_panic_pref(Some(learned));
        self.status = format!("Panic: {}.", learned.trigger());
        true
    }

    /// Saves the panic trigger and sends it to the engine. `None` clears it.
    pub(crate) fn set_panic_pref(&mut self, panic: Option<PanicPref>) {
        if panic.is_none() {
            self.rig.panic_learn = None;
        }
        let mut preferences = self.rig.store.saved().clone();
        preferences.panic = panic;
        if let Some(error) = self.rig.store.save(preferences).err() {
            self.status = error;
        }
        self.send_panic_trigger();
    }

    /// What the Preferences window shows for the trigger.
    pub(crate) fn panic_line(&self) -> String {
        if self.rig.panic_learn.is_some() {
            return "Waiting for MIDI...".to_string();
        }
        match self.rig.store.saved().panic {
            Some(panic) => panic.trigger().to_string(),
            None => "None".to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
