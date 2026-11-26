//! A custom element's pointer: the events it hears in the hit order, the
//! capture it may hold through a drag, and the cancels and the leave it
//! is told of (ui-layer.md, section 9, rule 1).
//!
//! While an element holds the capture it hears every pointer event first,
//! inside or outside its bounds, over any layer, and outside the window,
//! until every button is up; nothing else hovers or clicks meanwhile. The
//! wheel is not captured. A capture that ends any other way than by the
//! last release ends with [`CustomEvent::CaptureLost`], so the element
//! undoes the gesture rather than finishing it.

use super::{Capture, CustomEvent, Moment, Used};
use crate::app::App;
use crate::draw::Panels;
use crate::pointer::Heard;
use crate::view::Layer;
use loom_ui_core::{Point, PointerButton, PointerEvent, UiId};

/// The slot of `button` in [`Capture::held`].
fn slot(button: PointerButton) -> usize {
    match button {
        PointerButton::Left => 0,
        PointerButton::Right => 1,
        PointerButton::Middle => 2,
        PointerButton::Other => 3,
    }
}

impl<A: App> Panels<'_, A> {
    /// The moment for the element `id` with the pointer at `pointer`, or
    /// `None` when no custom element has that UI ID or no turn has run yet
    /// to give the time. A layout runs only after a turn, so an element
    /// that exists always has one.
    pub(super) fn custom_moment(&self, id: &UiId, pointer: Option<Point>) -> Option<Moment> {
        Some(Moment {
            pointer,
            bounds: self.customs.bounds(id)?,
            now: self.pointer.now?,
            modifiers: self.pointer.modifiers,
        })
    }

    /// Gives `event` to the element that holds the capture, as the first
    /// step for every pointer event but the wheel. `None` when no element
    /// holds it, or for a wheel, so the event is routed as any other. A
    /// release of the last button held ends the capture after the element
    /// hears it, and [`PointerEvent::Lost`] ends it with a cancel.
    pub(crate) fn offer_captured(&mut self, event: &PointerEvent, point: Option<Point>) -> Option<Heard> {
        let mut capture = self.customs.captured.clone()?;
        if matches!(event, PointerEvent::Scrolled(_)) {
            return None;
        }
        // The driver reports the buttons ended with no release: a cancel.
        if matches!(event, PointerEvent::Lost) {
            self.cancel_capture();
            return Some(Heard {
                taken: true,
                redraw: false,
            });
        }
        // The capture holds back the hover, but not where the pointer is:
        // `None` once it left the window. What it held back is applied
        // when the capture ends.
        self.pointer.at = point;
        let Some(moment) = self.custom_moment(&capture.id, point) else {
            self.customs.captured = None;
            self.settle_hover();
            return None;
        };
        match event {
            PointerEvent::Pressed(button) => capture.held[slot(*button)] += 1,
            PointerEvent::Released(button) => {
                let held = &mut capture.held[slot(*button)];
                *held = held.saturating_sub(1);
            }
            _ => {}
        }
        let _ = self
            .customs
            .tell(self.app, &capture.id, &CustomEvent::Pointer(*event), &moment);
        let released = matches!(event, PointerEvent::Released(_)) && capture.held.iter().all(|held| *held == 0);
        self.customs.captured = (!released).then_some(capture);
        let redraw = released && self.settle_hover();
        Some(Heard { taken: true, redraw })
    }

    /// Gives `event`, at `point`, to the custom element `id` of `layer`
    /// that the hit order reached, and starts a capture when it asks for
    /// one on a press. An element hovered before it hears that the pointer
    /// left first.
    pub(crate) fn offer_custom(&mut self, layer: Layer, id: &UiId, event: &PointerEvent, point: Point) -> Used {
        if matches!(event, PointerEvent::Moved | PointerEvent::Pressed(_)) {
            if self.customs.hovered.as_ref().is_some_and(|hovered| hovered != id) {
                self.tell_left();
            }
            self.customs.hovered = Some(id.clone());
        }
        let Some(moment) = self.custom_moment(id, Some(point)) else {
            return Used::No;
        };
        let used = self.customs.tell(self.app, id, &CustomEvent::Pointer(*event), &moment);
        if used == Used::Capture
            && let PointerEvent::Pressed(button) = event
        {
            let mut held = [0; 4];
            held[slot(*button)] = 1;
            self.customs.captured = Some(Capture {
                id: id.clone(),
                layer,
                held,
            });
        }
        used
    }

    /// Tells the hovered custom element that the pointer left it, and
    /// forgets it.
    fn tell_left(&mut self) {
        let Some(id) = self.customs.hovered.take() else {
            return;
        };
        if let Some(moment) = self.custom_moment(&id, self.pointer.at) {
            let _ = self.customs.tell(self.app, &id, &CustomEvent::PointerLeft, &moment);
        }
    }

    /// Tells a custom element told of the pointer over it that the pointer
    /// left, once, when it is no longer the hovered element: the pointer
    /// moved off, left the window, or a layer above took it. An element
    /// that holds the capture keeps the pointer, and hears it when the
    /// capture ends. Checked after routing, after each turn, and after
    /// each layout, as [`Panels::report_unhover`] is.
    pub(crate) fn report_custom_left(&mut self) {
        let Some(told) = &self.customs.hovered else {
            return;
        };
        let still = self.pointer.hovered.as_ref().is_some_and(|(_, id)| id == told);
        if still || self.customs.captured() == Some(told) {
            return;
        }
        self.tell_left();
    }

    /// Ends the capture with a cancel: the element hears
    /// [`CustomEvent::CaptureLost`] and no release. Returns true when an
    /// element held it.
    pub(crate) fn cancel_capture(&mut self) -> bool {
        let Some(capture) = self.customs.captured.take() else {
            return false;
        };
        if let Some(moment) = self.custom_moment(&capture.id, self.pointer.at) {
            let _ = self
                .customs
                .tell(self.app, &capture.id, &CustomEvent::CaptureLost, &moment);
        }
        self.settle_hover();
        true
    }

    /// Applies the hover a capture held back, once it ended: outside the
    /// window, the pointer left it; inside, the element now under the
    /// pointer is hovered, in the layers that take input, and an element
    /// the pointer is no longer over hears it left, in this same turn.
    /// Returns true when the hover changed.
    fn settle_hover(&mut self) -> bool {
        let changed = match self.pointer.at {
            None => self.pointer.clear(),
            Some(point) => {
                let input = self.app.input().layer;
                let at = self
                    .window
                    .hit_in(point, |layer| layer >= input)
                    .and_then(|(layer, hit)| match hit {
                        crate::hit::Hit::Element(id) => Some((layer, id.clone())),
                        crate::hit::Hit::Blocked => None,
                    });
                let changed = self.pointer.set_hover(at);
                self.report_hover(changed);
                changed
            }
        };
        self.report_custom_left();
        changed
    }

    /// Cancels the capture when the input layer rose above the layer of
    /// the element that holds it, as an overlay or a question that opens
    /// mid-drag does: the gesture never finishes under a layer that now
    /// has the input (section 5, rule 1).
    pub(crate) fn cancel_capture_below(&mut self, input: Layer) -> bool {
        if self
            .customs
            .captured
            .as_ref()
            .is_some_and(|capture| capture.layer < input)
        {
            return self.cancel_capture();
        }
        false
    }

    /// Sends what elements sent, outside a turn, through the waker, so it
    /// runs on the next turn: a driver that offers an event itself, as one
    /// whose toolkit answers late does, has no turn to run it in.
    pub(crate) fn send_custom_later(&mut self) {
        let sent = self.customs.take_sent();
        if !sent.is_empty() {
            self.waker.send_all(sent);
        }
    }
}

#[cfg(test)]
mod tests;
