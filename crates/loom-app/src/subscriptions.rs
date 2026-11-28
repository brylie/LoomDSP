//! What the runtime hands the app about the window: the window events
//! ([`window_message`]), help's keys ([`help_key`]), and the lights on
//! screen the telemetry watches. The watcher threads are in `watching.rs`,
//! and the timers and frames are deadlines (`timers.rs`).

use crate::{App, FileAction, Message, lights};
use loom_core::ModuleId;
use loom_ui_core::{Event, Key, KeyEvent};

impl App {
    /// The lights on screen and where their values live, with the reader of
    /// this engine's table, or `None` when there are none.
    pub(crate) fn watched_lights(&self) -> Option<lights::OnScreen> {
        let graph = self.session.controller.graph();
        let lights: Vec<lights::Entry> = self
            .session
            .visible_displays
            .iter()
            .filter_map(|&id| Some((id, graph.node(ModuleId(id))?.descriptor)))
            .flat_map(|(id, d)| {
                d.panel.widgets.iter().enumerate().filter_map(move |(index, w)| {
                    let display = (!w.is_control()).then(|| d.display_index(w.bind()))??;
                    Some(((id, index), display, lights::range(d, w), lights::readout(d, w)))
                })
            })
            .filter_map(|(light, display, range, readout)| {
                let source = self.session.controller.display_source(ModuleId(light.0), display)?;
                Some(lights::Entry {
                    light,
                    source,
                    range,
                    readout,
                })
            })
            .collect();
        (!lights.is_empty()).then(|| lights::OnScreen {
            reader: self.session.controller.display_reader(),
            lights,
        })
    }

    /// The trigger lights on the watched list now.
    fn watched_triggers(&self) -> std::collections::BTreeSet<lights::Light> {
        self.watched_lights()
            .into_iter()
            .flat_map(|on| on.lights)
            .filter(|entry| entry.source.kind() == loom_core::DisplayKind::Trigger)
            .map(|entry| entry.light)
            .collect()
    }

    /// Whether a light shows a trigger display value, by its module's
    /// descriptor. A light that no longer exists is not one.
    fn is_trigger_light(&self, (id, widget): lights::Light) -> bool {
        let Some(node) = self.session.controller.graph().node(ModuleId(id)) else {
            return false;
        };
        let d = node.descriptor;
        d.panel
            .widgets
            .get(widget)
            .filter(|w| !w.is_control())
            .and_then(|w| d.display_index(w.bind()))
            .and_then(|index| d.displays.get(index))
            .is_some_and(|spec| spec.kind == loom_core::DisplayKind::Trigger)
    }

    /// Takes the light readings the watcher thread sent. A reading for a
    /// trigger light off the watched list is ignored: it was sent before the
    /// light left the screen, and would light it again after
    /// [`App::set_visible_displays`] set it dark. A level light takes every
    /// reading, as its last value is the best guess until the next one
    /// (`docs/design/panels.md`, Redraw).
    pub(crate) fn take_lights(&mut self, readings: Vec<(lights::Light, f32)>) {
        let mut watched = None;
        for (light, value) in readings {
            if self.is_trigger_light(light) && !watched.get_or_insert_with(|| self.watched_triggers()).contains(&light)
            {
                continue;
            }
            self.session.lights.insert(light, value);
        }
    }

    /// Says which modules' displays are on screen, as the canvas reports. A
    /// trigger light that leaves the screen is set dark, and so is one new
    /// to the list, so it does not draw lit for a frame when it comes back,
    /// before its first reading. A level light keeps its last value, the
    /// best guess until then (`docs/design/panels.md`, Redraw).
    pub(crate) fn set_visible_displays(&mut self, ids: Vec<u32>) {
        let before = self.watched_triggers();
        self.session.visible_displays = ids;
        let after = self.watched_triggers();
        for light in before.symmetric_difference(&after) {
            self.session.lights.insert(*light, 0.0);
        }
    }
}

/// An Escape nothing took closes help. The keys that open help are heard
/// first, whoever has the keys (`App::always_key`).
pub(crate) fn help_key(event: &Event, _used: bool) -> Option<Message> {
    match event {
        Event::Key(KeyEvent::Pressed { key: Key::Escape, .. }) => Some(Message::CloseHelp),
        _ => None,
    }
}

/// Whether a press opens help: F1, or Cmd+/ on macOS and Ctrl+/ elsewhere,
/// with or without Shift, so Cmd+? is the same key (ui-layer.md, section 6,
/// rule 13). F1 needs fn on most laptops. The slash is found by the key or
/// by what the press types, since it moves between layouts: a German
/// layout types `/` with Shift and 7 and `?` with Shift and ß, a French one
/// `?` with Shift and the comma key, and on macOS the typed text keeps
/// Shift's effect while Cmd is held.
pub(crate) fn opens_help(key: Key, modifiers: loom_ui_core::Modifiers, text: Option<&str>) -> bool {
    key == Key::F1
        || (modifiers.shortcut() && !modifiers.alt && (key == Key::Character('/') || matches!(text, Some("/" | "?"))))
}

/// The app's message for a window event the runtime hands it, if it wants
/// one: the close request becomes the app's close, so the window stays open
/// until the app has asked about unsaved work (files.md); a dropped file
/// becomes a message, one per file, which the app collects for a moment;
/// a new scale redraws.
pub(crate) fn window_message(event: &loom_ui::WindowEvent) -> Option<Message> {
    match event {
        loom_ui::WindowEvent::CloseRequested => Some(Message::File(FileAction::Close)),
        loom_ui::WindowEvent::FileDropped(path) => Some(Message::FileDropped(path.clone())),
        loom_ui::WindowEvent::ScaleChanged(_) => Some(Message::WindowRescaled),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
