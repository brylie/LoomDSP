//! Pointer events in the runtime's own layers (ui-layer.md, section 5,
//! rule 10).
//!
//! While a toolkit still draws part of the window, the runtime's layers sit
//! on both sides of it: Overlays and Question above the routing target, and
//! Canvas and Panels below it. A pointer event is offered from the top
//! down: the layers above, then the target, then the layers below. Each
//! step here is one function, so the runtime's own routing and a driver
//! that hears the target's answer late ([`Routed::Later`]) run the same
//! code.
//!
//! What a layer that takes an event does with it is widget state: the
//! element under the pointer is hovered, a press focuses the stop there
//! with no ring (section 6, rule 3), and a left press and a left release on
//! the same element are a click, which the app turns into a message
//! ([`App::clicked`]). The press holds while the pointer leaves the element
//! and comes back, so that is still a click; a release anywhere else
//! cancels it (Brylie's decision, 2026-11-17). A click's message runs on
//! the next turn, whichever path found it. Hover and pressed run no update
//! (section 3, rule 4).
//!
//! [`Routed::Later`]: crate::Routed::Later

use crate::app::{App, InputState};
use crate::draw::Panels;
use crate::hit::Hit;
use crate::view::Layer;
use loom_ui_core::{Event, Point, PointerButton, PointerEvent, UiId};

/// The pointer's widget state among the runtime's elements.
#[derive(Debug, Default)]
pub(crate) struct Pointer {
    /// The element under the pointer, and its layer: the one hover text
    /// reads. At most one layer in one toolkit holds the hover.
    pub(crate) hovered: Option<(Layer, UiId)>,
    /// The element a left press went down on, and its layer, until the
    /// release.
    pub(crate) pressed: Option<(Layer, UiId)>,
    /// The focused element Space went down on, which clicks when Space
    /// comes up there (section 3, rule 4). A pointer press cancels it.
    pub(crate) space: Option<UiId>,
    /// What chooses the hover text, between the pointer and the focus.
    pub(crate) hint: crate::hint::Chooser,
}

impl Pointer {
    /// Forgets the hover and the press: the pointer left the window.
    /// Another toolkit taking a move ends only the hover, so a press held
    /// across it still clicks. Returns true when the hover changed.
    pub(crate) fn clear(&mut self) -> bool {
        self.pressed = None;
        self.space = None;
        self.hovered.take().is_some()
    }

    /// Forgets the hover and the press in layers below `input`, which take
    /// no input now. Returns true when the hover changed.
    pub(crate) fn below(&mut self, input: Layer) -> bool {
        if self.pressed.as_ref().is_some_and(|(layer, _)| *layer < input) {
            self.pressed = None;
        }
        if self.hovered.as_ref().is_some_and(|(layer, _)| *layer < input) {
            self.hovered = None;
            return true;
        }
        false
    }
}

/// What one of the runtime's layers did with a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use]
pub struct Heard {
    /// A layer took it, so nothing below it hears it.
    pub taken: bool,
    /// The hover or the focus moved, so the window needs drawing. A driver
    /// that heard this outside a turn asks for a frame: that frame's turn
    /// also carries out what the move owes the target and the app.
    pub redraw: bool,
}

/// Which side of the routing target a step offers an event to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// Overlays and Question: before the target.
    Above,
    /// Canvas and Panels: after the target, the last step.
    Below,
}

impl Side {
    /// The side `layer` is on.
    fn of(layer: Layer) -> Self {
        if layer >= Layer::Overlays {
            Self::Above
        } else {
            Self::Below
        }
    }
}

impl<A: App> Panels<'_, A> {
    /// True when one of the runtime's layers above the target takes a
    /// pointer at `point`: a driver whose toolkit routes before the turn
    /// asks this first, and keeps the event from the toolkit when it is
    /// true (a move reaches it as the pointer leaving).
    pub fn over_target(&self, event: &Event, point: Point) -> bool {
        let input = self.app.input().layer;
        self.keeps_hint(event, point)
            || self
                .window
                .hit_in(point, |layer| Side::of(layer) == Side::Above && layer >= input)
                .is_some()
    }

    /// True for a move onto the hover text showing, or between it and its
    /// element, which keeps it showing (section 5, rule 11). It takes no
    /// other event: a press there goes where it would go without it.
    fn keeps_hint(&self, event: &Event, point: Point) -> bool {
        matches!(event, Event::Pointer(PointerEvent::Moved))
            && self.window.hint().is_some_and(|hint| hint.keep.contains(point))
    }

    /// Offers a pointer event at `point` to the runtime's layers below the
    /// target, for a driver to call when its target answered
    /// [`Routed::Later`](crate::Routed::Later) and then left this event
    /// unused. A click's message is sent for the next turn.
    pub fn pointer_below(&mut self, event: &Event, point: Point) -> Heard {
        let input = self.app.input();
        let heard = self.offer(input, event, point, Side::Below);
        self.report_focus();
        heard
    }

    /// Another toolkit took `event`, a pointer event none of the runtime's
    /// layers heard. The hover clears, so one layer in one toolkit holds
    /// it. A press stays through a move: dragging across the other toolkit
    /// and coming back still clicks. A left release there cancels it, and
    /// so does a new left press, which can only follow a release the
    /// runtime missed (section 3, rule 4).
    pub fn pointer_taken(&mut self, event: &Event) -> Heard {
        if matches!(
            event,
            Event::Pointer(PointerEvent::Pressed(PointerButton::Left) | PointerEvent::Released(PointerButton::Left))
        ) {
            self.pointer.pressed = None;
        }
        if matches!(event, Event::Pointer(PointerEvent::Pressed(_))) {
            self.pointer.space = None;
        }
        Heard {
            taken: false,
            redraw: self.pointer.hovered.take().is_some(),
        }
    }

    /// Offers a pointer event to the runtime's layers above the target.
    pub(crate) fn pointer_above(&mut self, input: InputState, event: &Event, point: Point) -> Heard {
        self.offer(input, event, point, Side::Above)
    }

    /// Offers `event` to the layers on `side` that take input now, and
    /// carries out what the one that takes it does. When none takes it in
    /// the last step, below the target, a move clears the hover and a
    /// release ends the press, as nothing of the runtime's is under the
    /// pointer. The step above leaves them for the steps after it. A move
    /// off the pressed element keeps the press, so coming back and letting
    /// go there still clicks.
    fn offer(&mut self, input: InputState, event: &Event, point: Point, side: Side) -> Heard {
        let Event::Pointer(pointer) = event else {
            return Heard::default();
        };
        if side == Side::Above && self.keeps_hint(event, point) {
            // The hover stays on the hint's element, so the hint stays.
            return Heard {
                taken: true,
                redraw: false,
            };
        }
        let hit = self
            .window
            .hit_in(point, |layer| Side::of(layer) == side && layer >= input.layer)
            .map(|(layer, hit)| match hit {
                Hit::Element(id) => (layer, Some(id.clone())),
                Hit::Blocked => (layer, None),
            });
        let Some((layer, id)) = hit else {
            if side == Side::Above {
                return Heard::default();
            }
            let redraw = match pointer {
                PointerEvent::Moved => self.pointer.hovered.take().is_some(),
                PointerEvent::Released(PointerButton::Left) => {
                    self.pointer.pressed = None;
                    false
                }
                PointerEvent::Pressed(_) => {
                    self.pointer.space = None;
                    false
                }
                _ => false,
            };
            return Heard { taken: false, redraw };
        };
        let at = id.map(|id| (layer, id));
        let mut redraw = false;
        match pointer {
            PointerEvent::Moved => redraw = self.hover(at),
            PointerEvent::Pressed(button) => {
                self.pointer.space = None;
                redraw = self.hover(at.clone());
                let before = self.focus.id.clone();
                self.focus_in(point, |layer| Side::of(layer) == side && layer >= input.layer);
                redraw |= self.focus.id != before;
                if *button == PointerButton::Left {
                    self.pointer.pressed = at;
                }
            }
            PointerEvent::Released(PointerButton::Left) => {
                if let Some(pressed) = self.pointer.pressed.take()
                    && Some(&pressed) == at.as_ref()
                    && let Some(message) = self.app.clicked(&pressed.1)
                {
                    self.waker.send(message);
                }
            }
            _ => {}
        }
        Heard { taken: true, redraw }
    }

    /// Moves the hover to `at`. Returns true when it changed.
    fn hover(&mut self, at: Option<(Layer, UiId)>) -> bool {
        let changed = self.pointer.hovered != at;
        if changed {
            self.pointer.hint.latest = crate::hint::Latest::Pointer;
        }
        self.pointer.hovered = at;
        changed
    }

    /// Focuses the stop a press at `point` reaches, with no ring, and owes
    /// the target its release when a stop takes it, as
    /// [`Runtime::focus_at`](crate::Runtime::focus_at) does.
    pub(crate) fn focus_at(&mut self, point: Point) {
        self.focus_in(point, |_| true);
    }

    /// [`Panels::focus_at`] among the panels of the layers `keep` allows,
    /// so a press focuses in the same panels it hits.
    fn focus_in(&mut self, point: Point, keep: impl Fn(Layer) -> bool) {
        self.focus.wanted = None;
        self.focus.id = self.window.focus_at_in(point, keep).flatten().cloned();
        self.focus.ring = false;
        if self.focus.id.is_some() {
            self.focus.release_owed = true;
        }
    }
}

#[cfg(test)]
mod tests;
