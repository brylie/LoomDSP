//! Pointer events between Iced and the runtime's own layers (ui-layer.md,
//! section 5, rule 10), for the window and the `Ui` harness alike.
//!
//! Iced hears the window's events before the runtime's turn and answers
//! after it, so the bridge routes in two halves:
//!
//! - [`Pointers::before`], as each event comes: a press or a release over
//!   one of the runtime's layers above Iced never reaches Iced, and a move
//!   there reaches it as the pointer leaving, once;
//! - [`Pointers::update`], after the runtime's turn: the keys the runtime
//!   took are left out, the release the runtime owes Iced runs first, then
//!   Iced's update over the events kept, and then each pointer event Iced
//!   left unused goes to the runtime's layers below it, while a move Iced
//!   used takes the hover from them.
//!
//! The window (`window.rs`) and the harness (`ui_test.rs`) both call these,
//! so what the tests check is what the window does.

use super::{Bridged, Hosted, IcedTarget};
use iced::Task;
use iced::event::Status;
use iced::mouse;
use loom_ui::Panels;
use loom_ui_core::{Event, Point, PointerButton, PointerEvent};

/// The pointer events kept for Iced since its last update, and what the
/// runtime's layers need to hear of them after it.
#[derive(Default)]
pub(crate) struct Pointers {
    /// Where the pointer is, from the last move, in logical pixels.
    cursor: Option<Point>,
    /// Iced was told the pointer left, because one of the runtime's layers
    /// above it holds the pointer.
    hidden: bool,
    /// For each event kept for Iced, in order: the pointer event it is in
    /// `loom-ui`'s terms and where, or `None` for any other event.
    kept: Vec<Option<(Event, Point)>>,
}

impl Pointers {
    /// Keeps `event` for Iced in `events`, unless one of the runtime's
    /// layers above Iced takes its point. A press or a release there is
    /// left out, and a move there becomes Iced's pointer leaving, once;
    /// the next move outside them brings it back. Returns the event as the
    /// runtime routes it, with its point, when it is one of the pointer's:
    /// the window's shell hands the runtime its own, and the harness this.
    pub(crate) fn before(
        &mut self,
        panels: &Panels<'_, Hosted>,
        event: iced::Event,
        events: &mut Vec<iced::Event>,
    ) -> Option<(Event, Point)> {
        match &event {
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                self.cursor = Some(Point::new(position.x, position.y));
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => self.cursor = None,
            _ => {}
        }
        let pointer = ours(&event).zip(self.cursor);
        if let Some((loom, point)) = &pointer {
            if panels.over_target(loom, *point) {
                if !std::mem::replace(&mut self.hidden, true) {
                    self.keep(iced::Event::Mouse(mouse::Event::CursorLeft), None, events);
                }
                return pointer;
            }
            if std::mem::take(&mut self.hidden) {
                self.keep(iced::Event::Mouse(mouse::Event::CursorEntered), None, events);
            }
        }
        self.keep(event, pointer.clone(), events);
        pointer
    }

    /// Leaves the kept event at `index` out of what Iced hears: the key
    /// whose turn the runtime took (`IcedTarget::kept`), and nothing else,
    /// so a modifier change or another key in the same batch still reaches
    /// Iced. The window passes its batch's last event, since one report
    /// follows each window event; the harness passes the event whose turn
    /// just ran.
    pub(crate) fn drop_key_at(&mut self, index: usize, events: &mut Vec<iced::Event>) {
        debug_assert!(
            matches!(
                events.get(index),
                Some(iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { .. }))
            ),
            "only a pressed key is kept from Iced"
        );
        if index < events.len() {
            events.remove(index);
            self.kept.remove(index);
        }
    }

    /// Keeps one event for Iced, with what the runtime's layers hear of it.
    fn keep(&mut self, event: iced::Event, pointer: Option<(Event, Point)>, events: &mut Vec<iced::Event>) {
        events.push(event);
        self.kept.push(pointer);
    }

    /// One Iced update over `events`, the events kept since the last, as
    /// the window's report and the harness both run it. The order is the
    /// point: the release the runtime owes Iced runs before Iced hears the
    /// events, so a key after a focus our elements took never reaches the
    /// field that had it (section 6, rule 1). Returns true when the window
    /// needs drawing.
    pub(crate) fn update(
        &mut self,
        mut events: Vec<iced::Event>,
        target: &mut IcedTarget,
        panels: &mut Panels<'_, Hosted>,
        iced: &mut dyn IcedSide,
    ) -> bool {
        if target.take_drop_keys()
            && let Some(last) = events.len().checked_sub(1)
        {
            self.drop_key_at(last, &mut events);
        }
        if let Some(release) = target.take_release() {
            iced.run(release);
        }
        let statuses = if events.is_empty() { None } else { iced.update(&events) };
        let redraw = match statuses {
            Some(statuses) => self.after(panels, &statuses),
            None => {
                self.kept.clear();
                false
            }
        };
        // A script's pointer events go into Iced during that update (the
        // control channel's injector); what Iced left goes on to our layers.
        redraw | hand_scripted(iced.take_scripted(), panels)
    }

    /// After Iced's update over the events kept, with its `statuses` in the
    /// same order: each pointer event Iced ignored goes to the runtime's
    /// layers below it, and one Iced captured takes the hover from them. A
    /// click there runs on the next turn. Returns true when the window
    /// needs drawing.
    fn after(&mut self, panels: &mut Panels<'_, Hosted>, statuses: &[Status]) -> bool {
        let mut redraw = false;
        for (pointer, status) in std::mem::take(&mut self.kept).iter().zip(statuses) {
            let Some((event, point)) = pointer else { continue };
            let heard = match status {
                Status::Ignored => panels.pointer_below(event, *point),
                Status::Captured => panels.pointer_taken(event),
            };
            redraw |= heard.redraw;
        }
        redraw
    }
}

/// Where Iced runs, for [`Pointers::update`]: the window, or the harness.
pub(crate) trait IcedSide {
    /// Runs a task on Iced's widgets now.
    fn run(&mut self, task: Task<Bridged>);

    /// Runs Iced's update over `events`, sends on the messages it gives,
    /// and returns each event's status, in order. `None` when Iced cannot
    /// run yet, such as before the window has a renderer.
    fn update(&mut self, events: &[iced::Event]) -> Option<Vec<Status>>;

    /// The scripted pointer events Iced's widgets left unused in that
    /// update, with where they happened.
    fn take_scripted(&mut self) -> Vec<(iced::Event, iced::Point)>;
}

/// Hands the control channel's scripted pointer events that Iced left
/// unused to the runtime's layers below Iced, as [`Pointers::update`] hands
/// it a real one (ui-layer.md, section 5, rule 10), so a script presses
/// the stats bar's × as a user does. Returns true when the window needs
/// drawing.
pub(crate) fn hand_scripted(scripted: Vec<(iced::Event, iced::Point)>, panels: &mut Panels<'_, Hosted>) -> bool {
    let mut redraw = false;
    for (event, at) in scripted {
        if let Some(event) = ours(&event) {
            redraw |= panels.pointer_below(&event, Point::new(at.x, at.y)).redraw;
        }
    }
    redraw
}

/// An Iced mouse event as the runtime's layers hear it: a move, or a press
/// or a release of a button they know. Scrolls stay Iced's until a layer
/// of the runtime's scrolls.
fn ours(event: &iced::Event) -> Option<Event> {
    let button = |button: &mouse::Button| match button {
        mouse::Button::Left => Some(PointerButton::Left),
        mouse::Button::Right => Some(PointerButton::Right),
        mouse::Button::Middle => Some(PointerButton::Middle),
        _ => None,
    };
    let pointer = match event {
        iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => PointerEvent::Moved,
        iced::Event::Mouse(mouse::Event::ButtonPressed(pressed)) => PointerEvent::Pressed(button(pressed)?),
        iced::Event::Mouse(mouse::Event::ButtonReleased(released)) => PointerEvent::Released(button(released)?),
        _ => return None,
    };
    Some(Event::Pointer(pointer))
}

#[cfg(test)]
mod tests;
