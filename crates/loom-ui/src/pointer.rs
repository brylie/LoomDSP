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
//! (section 3, rule 4), except that an element that asks hears the pointer
//! coming onto it ([`App::hovered`]). A right press on an element with a
//! context menu opens it ([`App::context_menu`], rule 19).
//!
//! A scroll container's bar and the wheel are the pointer's too (section 5,
//! rule 12). The wheel scrolls the innermost container under the pointer,
//! and what it cannot take passes out, then on to whatever is next. It
//! never moves a slider, with or without a modifier (rule 16). A press on
//! the thumb drags it until the release, wherever the pointer goes; a press
//! above or below it moves one page toward the press.
//!
//! A left press on a slider drags it until the release, wherever the
//! pointer goes, as a bar's thumb does; the drag reaches the app through
//! [`App::slid`] ([`crate::slider`]).
//!
//! [`Routed::Later`]: crate::Routed::Later

use crate::app::{App, InputState};
use crate::draw::Panels;
use crate::hit::Hit;
use crate::view::Layer;
use crate::window::ThumbDrag;
use loom_ui_core::{Event, Modifiers, Point, PointerButton, PointerEvent, ScrollDelta, UiId};
use std::time::Instant;

/// How many lines of body text one step of a wheel scrolls (section 5,
/// rule 12).
const WHEEL_LINES: f32 = 3.0;

/// The pointer's widget state among the runtime's elements.
#[derive(Debug, Default)]
pub(crate) struct Pointer {
    /// The element under the pointer, and its layer: the one hover text
    /// reads. At most one layer in one toolkit holds the hover.
    pub(crate) hovered: Option<(Layer, UiId)>,
    /// Counts hover changes, including ones between frames.
    pub(crate) hover_moves: u64,
    /// The element the app was last told the pointer came onto
    /// ([`App::hovered`]), until it is told the hover ended
    /// ([`App::unhovered`]), so each start has one end.
    pub(crate) told: Option<UiId>,
    /// The element a left press went down on, and its layer, until the
    /// release.
    pub(crate) pressed: Option<(Layer, UiId)>,
    /// The focused element Space went down on, and the focus's move count
    /// then: it clicks when Space comes up with the focus never moved
    /// since (section 3, rule 4). A pointer press cancels it.
    pub(crate) space: Option<(UiId, u64)>,
    /// What chooses the hover text, between the pointer and the focus.
    pub(crate) hint: crate::hint::Chooser,
    /// Where the pointer last moved, in the window, to check the hover
    /// again after a layout moves what was under it.
    pub(crate) at: Option<Point>,
    /// The press on a scroll container's bar that is held, until the
    /// release: the thumb it drags, if it went down on the thumb.
    pub(crate) thumb: Option<ThumbDrag>,
    /// The modifier keys held, as last reported: Shift during a slider's
    /// drag makes it finer.
    pub(crate) modifiers: Modifiers,
    /// The sliders' drag, group of keys, and last press (section 5, rule
    /// 16).
    pub(crate) slide: crate::slider::Sliding,
    /// The moment of the latest turn, for a double-click on a slider. The
    /// runtime sets it at the start of each turn, since it never reads the
    /// clock.
    pub(crate) now: Option<Instant>,
}

impl Pointer {
    /// Updates the hover and records transitions even if it returns before layout.
    pub(crate) fn set_hover(&mut self, hovered: Option<(Layer, UiId)>) -> bool {
        if self.hovered == hovered {
            return false;
        }
        self.hovered = hovered;
        self.hover_moves += 1;
        true
    }

    /// Forgets the hover and the press: the pointer left the window.
    /// Another toolkit taking a move ends only the hover, so a press held
    /// across it still clicks. A held Space stays, since it is the
    /// keyboard's and no focus moved. Returns true when the hover changed.
    pub(crate) fn clear(&mut self) -> bool {
        self.pressed = None;
        self.thumb = None;
        self.at = None;
        self.set_hover(None)
    }

    /// Forgets the hover and the press in layers below `input`, which take
    /// no input now. Returns true when the hover changed.
    pub(crate) fn below(&mut self, input: Layer) -> bool {
        if self.pressed.as_ref().is_some_and(|(layer, _)| *layer < input) {
            self.pressed = None;
        }
        if self.hovered.as_ref().is_some_and(|(layer, _)| *layer < input) {
            return self.set_hover(None);
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
    /// true (a move reaches it as the pointer leaving). A held press on a
    /// scroll container's bar keeps every pointer event until its release,
    /// and a select's open list every press (section 5, rule 18).
    pub fn over_target(&self, event: &Event, point: Point) -> bool {
        let input = self.app.input().layer;
        self.pointer.thumb.is_some()
            || (self.window.select.is_some() && matches!(event, Event::Pointer(PointerEvent::Pressed(_))))
            || self.keeps_hint(event, point)
            || !self.pressed_outside(input, event, point).is_empty()
            || self
                .window
                .hit_in(point, |layer| Side::of(layer) == Side::Above && layer >= input)
                .is_some()
    }

    /// The panels a press at `point` is outside of, when `event` is a press
    /// of any button and the input layer, above the target, has panels that
    /// close on one (section 5, rule 14): each such panel drawn over the
    /// topmost one the press is inside, or every one when it is inside
    /// none. A press on a browser closes the row menu over it and keeps the
    /// browser; a press on the menu keeps both, even where the menu hangs
    /// past the browser's edge. Empty otherwise.
    fn pressed_outside(&self, input: Layer, event: &Event, point: Point) -> Vec<UiId> {
        if !matches!(event, Event::Pointer(PointerEvent::Pressed(_))) || Side::of(input) != Side::Above {
            return Vec::new();
        }
        let roots = self.window.closing_roots(input);
        let over = roots
            .iter()
            .rposition(|(_, at)| at.contains(point))
            .map_or(0, |at| at + 1);
        roots.into_iter().skip(over).map(|(root, _)| root).collect()
    }

    /// True for a move onto the hover text showing, or between it and its
    /// element, which keeps it showing (section 5, rule 11). It takes no
    /// other event: a press there goes where it would go without it.
    /// It keeps only a hover the pointer has: while the pointer hovers the
    /// hint's element. A hint shown for the focus keeps nothing, so a move
    /// onto its element hovers it as any move does. A hint whose element is
    /// now below the input layer (a question opened before the next draw)
    /// keeps nothing either, since the update that raised the layer ended
    /// that hover (`Pointer::below`).
    fn keeps_hint(&self, event: &Event, point: Point) -> bool {
        matches!(event, Event::Pointer(PointerEvent::Moved))
            && self.window.hint().is_some_and(|hint| {
                self.pointer.hovered.as_ref().is_some_and(|(_, id)| *id == hint.source)
                    && self
                        .window
                        .bounds(&hint.source)
                        .is_some_and(|anchor| hint.keeps(point, anchor))
            })
    }

    /// Offers a pointer event at `point` to the runtime's layers below the
    /// target, for a driver to call when its target answered
    /// [`Routed::Later`](crate::Routed::Later) and then left this event
    /// unused. A click's message is sent for the next turn.
    ///
    /// It runs outside a turn, so it reports a change of what shows as
    /// pressed itself, in [`Heard::redraw`]: the turn's own comparison
    /// never sees it.
    pub fn pointer_below(&mut self, event: &Event, point: Point) -> Heard {
        let input = self.app.input();
        let pressed = self.shown_pressed();
        let mut heard = self.offer(input, event, point, Side::Below);
        heard.redraw |= self.shown_pressed() != pressed;
        self.report_focus();
        self.nudge_for_select();
        heard
    }

    /// Another toolkit took `event`, a pointer event none of the runtime's
    /// layers heard. The hover clears, so one layer in one toolkit holds
    /// it. A press stays through a move: dragging across the other toolkit
    /// and coming back still clicks. A left release there cancels it, and
    /// so does a new left press, which can only follow a release the
    /// runtime missed (section 3, rule 4).
    pub fn pointer_taken(&mut self, event: &Event) -> Heard {
        let pressed = self.shown_pressed();
        if matches!(
            event,
            Event::Pointer(PointerEvent::Pressed(PointerButton::Left) | PointerEvent::Released(PointerButton::Left))
        ) {
            self.pointer.pressed = None;
        }
        if matches!(event, Event::Pointer(PointerEvent::Pressed(_))) {
            self.pointer.space = None;
        }
        let hover = self.pointer.set_hover(None);
        Heard {
            taken: false,
            redraw: hover || self.shown_pressed() != pressed,
        }
    }

    /// The element that shows as pressed (visual-system.md, Role tokens):
    /// the one a held left press went down on while the pointer is still
    /// over it, or the one a held Space would click now
    /// ([`live_arm`](crate::runtime::live_arm)), so the look says exactly
    /// what letting go does. `None` when neither holds.
    pub(crate) fn shown_pressed(&self) -> Option<UiId> {
        if let Some(armed) = crate::runtime::live_arm(self.window, self.app, self.focus, self.pointer) {
            return Some(armed);
        }
        let (_, pressed) = self.pointer.pressed.as_ref()?;
        let over = self
            .pointer
            .hovered
            .as_ref()
            .is_some_and(|(_, hovered)| hovered == pressed);
        over.then(|| pressed.clone())
    }

    /// True when the element with UI ID `id` is a menu item.
    fn is_menu_item(&self, id: &UiId) -> bool {
        self.window
            .access_of(id)
            .is_some_and(|access| access.role() == crate::access::Role::MenuItem)
    }

    /// True when the element with UI ID `id` is a turned-off menu item,
    /// which a click picks nothing on, though it may be a focus stop
    /// ([`Element::focusable_while_off`](crate::Element::focusable_while_off)).
    fn is_off_menu_item(&self, id: &UiId) -> bool {
        self.window
            .access_of(id)
            .is_some_and(|access| access.role() == crate::access::Role::MenuItem && access.is_disabled())
    }

    /// True when the element with UI ID `id` is a button, a switch, a
    /// check box, a link, or a menu item, which a pointer press does not
    /// focus: each acts, and none takes input the keys would then go to
    /// (section 6, rule 3; section 5, rule 17).
    fn takes_no_press_focus(&self, id: &UiId) -> bool {
        self.window.access_of(id).is_some_and(|access| {
            matches!(
                access.role(),
                crate::access::Role::Button
                    | crate::access::Role::Switch
                    | crate::access::Role::CheckBox
                    | crate::access::Role::Link
                    | crate::access::Role::MenuItem
            )
        })
    }

    /// Offers a pointer event to the runtime's layers above the target, as
    /// the runtime's own routing does first, for a driver that delivers an
    /// event itself outside a turn, such as a script's: a press outside a
    /// closing menu is taken and told, and one on a dialog's scrim is held.
    pub fn pointer_above_target(&mut self, event: &Event, point: Point) -> Heard {
        let input = self.app.input();
        let heard = self.pointer_above(input, event, point);
        self.report_focus();
        self.nudge_for_select();
        heard
    }

    /// Sends the app's message for each of `reports` from a slider, for
    /// the next turn ([`App::slid`]). A focus move not yet told goes
    /// first: a press on a slider both focuses it and moves it, and the
    /// app handles the move with its copy of the focus current.
    pub(crate) fn report_slid(&mut self, reports: impl IntoIterator<Item = crate::slider::Slid>) {
        let mut reports = reports.into_iter().peekable();
        if reports.peek().is_some() {
            self.report_focus();
        }
        for (id, position, phase) in reports {
            if let Some(message) = self.app.slid(&id, position, phase) {
                self.waker.send(message);
            }
        }
    }

    /// Opens the list of the select `id` for a left press on it, on its
    /// chosen option, unless it has no options. Returns true when it was a
    /// select that took the press, which is no click. A press on an open
    /// select never comes here: it is outside the list, which closes.
    fn press_select(&mut self, id: &UiId) -> bool {
        let Some((select, _, _)) = self.window.select_of(id) else {
            return false;
        };
        if select.options.is_empty() {
            return false;
        }
        let at = select.start().unwrap_or(0);
        self.open_select(id, select, at);
        true
    }

    /// Wakes the loop for a turn when a select's list changed outside one,
    /// so the view is built again with it.
    fn nudge_for_select(&self) {
        if self.window.select_changed {
            self.waker.nudge();
        }
    }

    /// Starts a drag, or a reset, on the slider `id` for a left press at
    /// `point`, unless it is turned off. Returns true when it was a slider
    /// that took the press.
    fn press_slider(&mut self, id: &UiId, point: Point) -> bool {
        let Some((slider, bounds)) = self.window.slider_of(id) else {
            return false;
        };
        if self
            .window
            .access_of(id)
            .is_some_and(crate::access::Access::is_disabled)
        {
            return false;
        }
        let shift = self.pointer.modifiers.shift;
        let reports = self
            .pointer
            .slide
            .press(id, slider, bounds, point, shift, self.pointer.now);
        self.report_slid(reports);
        true
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
        if matches!(pointer, PointerEvent::Moved | PointerEvent::Pressed(_)) {
            self.pointer.at = Some(point);
        }
        let keep = |layer: Layer| Side::of(layer) == side && layer >= input.layer;
        // A held press on a bar has the pointer until it is let go, at the
        // first step that hears it, wherever the pointer is.
        if let Some(drag) = self.pointer.thumb.clone() {
            return match pointer {
                PointerEvent::Moved => Heard {
                    taken: true,
                    redraw: self.window.drag_thumb(&drag, point),
                },
                PointerEvent::Released(PointerButton::Left) => {
                    self.pointer.thumb = None;
                    Heard {
                        taken: true,
                        redraw: false,
                    }
                }
                _ => Heard {
                    taken: true,
                    redraw: false,
                },
            };
        }
        // A held drag on a slider has the pointer until it is let go, too.
        if self.pointer.slide.drags() {
            match pointer {
                PointerEvent::Moved => {
                    let shift = self.pointer.modifiers.shift;
                    let report = self.pointer.slide.moved(point, shift);
                    self.report_slid(report);
                }
                PointerEvent::Released(PointerButton::Left) => {
                    let report = self.pointer.slide.released();
                    self.report_slid(report);
                    self.pointer.pressed = None;
                }
                _ => {}
            }
            return Heard {
                taken: true,
                redraw: false,
            };
        }
        if let PointerEvent::Scrolled(delta) = pointer
            && let Some(heard) = self.wheel(*delta, point, keep)
        {
            return heard;
        }
        // Section 5, rule 18: a press anywhere outside a select's open list,
        // the select itself included, closes the list with no choice, and
        // is taken, as a press outside a menu is: before any scroll bar
        // takes it, though a press on the list's own bar is inside it.
        if side == Side::Above
            && matches!(pointer, PointerEvent::Pressed(_))
            && self.window.select.is_some()
            && !self
                .window
                .bounds(&crate::select::list_id())
                .is_some_and(|list| list.contains(point))
        {
            self.pointer.space = None;
            self.close_select();
            return Heard {
                taken: true,
                redraw: true,
            };
        }
        if let PointerEvent::Pressed(PointerButton::Left) = pointer
            && let Some(drag) = self.window.press_bar(point, keep)
        {
            self.pointer.space = None;
            self.pointer.thumb = Some(drag);
            return Heard {
                taken: true,
                redraw: true,
            };
        }
        if side == Side::Above && self.keeps_hint(event, point) {
            // Movement on the hint's source itself still counts as pointer input.
            let on_source = self
                .pointer
                .hovered
                .as_ref()
                .and_then(|(_, id)| self.window.bounds(id))
                .is_some_and(|bounds| bounds.contains(point));
            return Heard {
                taken: true,
                redraw: on_source && self.note_hover_motion(),
            };
        }
        // Section 5, rule 14: a press outside a panel of the input layer
        // that closes on one is taken, and told to the app.
        let outside = if side == Side::Above {
            self.pressed_outside(input.layer, event, point)
        } else {
            Vec::new()
        };
        if !outside.is_empty() {
            self.pointer.space = None;
            for root in &outside {
                if let Some(message) = self.app.pressed_outside(root) {
                    self.waker.send(message);
                }
            }
            return Heard {
                taken: true,
                redraw: false,
            };
        }
        let hit = self.window.hit_in(point, keep).map(|(layer, hit)| match hit {
            Hit::Element(id) => (layer, Some(id.clone())),
            Hit::Blocked => (layer, None),
        });
        let Some((layer, id)) = hit else {
            if side == Side::Above {
                return Heard::default();
            }
            let redraw = match pointer {
                PointerEvent::Moved => self.pointer.set_hover(None),
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
            PointerEvent::Moved => {
                let moves = self.pointer.hover_moves;
                redraw = self.hover(at.clone());
                self.report_hover(self.pointer.hover_moves != moves);
                // The pointer over an option of an open list highlights it.
                if let Some(index) = at.as_ref().and_then(|(_, id)| crate::select::option_index(id)) {
                    self.highlight_option(index);
                }
                // Section 6, rule 8: the pointer moving over a menu item
                // focuses it, with no ring, as native menus move their
                // highlight; the keys then go on from there. Only a menu
                // item that is a focus stop: one whose menu leaves the
                // focus in a field, as a browser's row menu does, keeps it
                // there.
                if let Some((_, id)) = &at
                    && self.is_menu_item(id)
                    && self.window.is_stop(id)
                    && self.focus.id() != Some(id)
                {
                    self.focus.wanted = None;
                    self.focus.set(Some(id.clone()));
                    self.focus.ring = false;
                    self.focus.release_owed = true;
                    redraw = true;
                }
            }
            PointerEvent::Pressed(button) => {
                self.pointer.space = None;
                let moves = self.pointer.hover_moves;
                redraw = self.hover(at.clone());
                self.report_hover(self.pointer.hover_moves != moves);
                // A right press on an element with a context menu opens it
                // (section 5, rule 19), after the press focused there.
                if *button == PointerButton::Right
                    && let Some((_, id)) = &at
                    && self.window.has_context_menu(id)
                    && let Some(message) = self.app.context_menu(id, false)
                {
                    self.waker.send(message);
                }
                let before = self.focus.id().cloned();
                // Over an overlay or a question, a press on no stop keeps
                // the focus where it was (section 6, rule 3).
                let modal = input.layer > Layer::Panels;
                self.focus_in(point, modal, |layer| Side::of(layer) == side && layer >= input.layer);
                redraw |= self.focus.id() != before.as_ref();
                // A press on a slider drags it or resets it, and is no click.
                if *button == PointerButton::Left {
                    let slider = at.as_ref().is_some_and(|(_, id)| {
                        let id = id.clone();
                        self.press_slider(&id, point) || self.press_select(&id)
                    });
                    if !slider {
                        self.pointer.pressed = at;
                    }
                }
            }
            PointerEvent::Released(PointerButton::Left) => {
                if let Some(pressed) = self.pointer.pressed.take()
                    && Some(&pressed) == at.as_ref()
                {
                    // A press and release on an option chooses it.
                    if let Some(index) = crate::select::option_index(&pressed.1) {
                        self.highlight_option(index);
                        self.choose_highlighted();
                    } else if self.is_off_menu_item(&pressed.1) {
                        // A turned-off menu item picks nothing, by the
                        // pointer as by a key (section 6, rule 2).
                    } else if let Some(message) = self.app.clicked(&pressed.1) {
                        self.waker.send(message);
                    }
                }
            }
            _ => {}
        }
        Heard { taken: true, redraw }
    }

    /// A wheel or a touch pad's scroll at `point`, in the layers `keep`
    /// allows: the containers under it take what they can. Taken only when
    /// one moved, so a wheel no container can use goes on. `None` with no
    /// container under the point: the event is then offered as any other
    /// is. Over a slider it scrolls too, since the wheel never moves one
    /// (section 5, rule 16).
    fn wheel(&mut self, delta: ScrollDelta, point: Point, keep: impl Fn(Layer) -> bool + Copy) -> Option<Heard> {
        // Positive is a scroll up, which moves the content down: a smaller
        // offset.
        let by = |line: f32| match delta {
            ScrollDelta::Lines { y, .. } => -y * WHEEL_LINES * line,
            ScrollDelta::Pixels { y, .. } => -y,
        };
        let moved = self.window.scroll_wheel(point, by, keep)?;
        Some(Heard {
            taken: moved,
            redraw: moved,
        })
    }

    /// Tells the app the pointer came onto the hovered element, when the
    /// hover just `changed` and that element asked to hear it (section 3,
    /// rule 4). Only the pointer's own move or press reports: a layout that
    /// moves rows under a still pointer, as typing in a search field does,
    /// moves the hover without telling the app, so the highlight stays
    /// where the keys left it.
    fn report_hover(&mut self, changed: bool) {
        if !changed {
            return;
        }
        self.report_unhover();
        if let Some((_, id)) = &self.pointer.hovered
            && self.window.reports_hover(id)
        {
            self.pointer.told = Some(id.clone());
            if let Some(message) = self.app.hovered(id) {
                self.waker.send(message);
            }
        }
    }

    /// Tells the app a hover it heard of ended (section 5, rule 11), once:
    /// when the element it was told of is no longer the hovered one, which
    /// covers the pointer moving to another element or off every element,
    /// leaving the window, and the input layer rising above it, or when
    /// that element left the layout. Checked after each turn and each
    /// layout, so every way the hover can change is heard the same way.
    pub(crate) fn report_unhover(&mut self) {
        let Some(told) = &self.pointer.told else {
            return;
        };
        let still = self.pointer.hovered.as_ref().is_some_and(|(_, id)| id == told) && self.window.reports_hover(told);
        if still {
            return;
        }
        let told = self.pointer.told.take().expect("checked above");
        if let Some(message) = self.app.unhovered(&told) {
            self.waker.send(message);
        }
    }

    /// Moves the hover to `at`. Returns true when it changed.
    /// A move onto an element, even the one already hovered, makes the
    /// pointer the latest input for hover text (section 5, rule 11).
    fn hover(&mut self, at: Option<(Layer, UiId)>) -> bool {
        let changed = self.pointer.set_hover(at);
        let new_winner = self.pointer.hovered.is_some() && self.note_hover_motion();
        changed || new_winner
    }

    /// Records pointer recency and requests a frame when it changes the visible hint.
    fn note_hover_motion(&mut self) -> bool {
        let moves = self.focus.moves;
        let was_last = self.pointer.hint.pointer_last(moves);
        self.pointer.hint.pointer_moved(moves);
        !was_last
            && self.pointer.hovered.as_ref().is_some_and(|(_, id)| {
                self.window.hover_text_of(id).is_some() && self.window.hint().is_some_and(|shown| &shown.source != id)
            })
    }

    /// Focuses the stop a press at `point` reaches, with no ring, and owes
    /// the target its release when a stop takes it, as
    /// [`Runtime::focus_at`](crate::Runtime::focus_at) does.
    pub(crate) fn focus_at(&mut self, point: Point) {
        self.focus_in(point, false, |_| true);
    }

    /// [`Panels::focus_at`] among the panels of the layers `keep` allows,
    /// so a press focuses in the same panels it hits. A button or a switch
    /// takes no focus from a press: the focus stays where it was, in
    /// either toolkit, as a push button on macOS leaves it (ui-layer.md,
    /// section 6, rule 3; Brylie's decision, 2026-11-19). Tab and a screen
    /// reader's Focus still reach it. A field, or a press on no stop,
    /// moves the focus as before.
    fn focus_in(&mut self, point: Point, modal: bool, keep: impl Fn(Layer) -> bool) {
        // A press in a select's open list leaves the focus on the select
        // (section 5, rule 18).
        if self.window.select.is_some()
            && self
                .window
                .bounds(&crate::select::list_id())
                .is_some_and(|list| list.contains(point))
        {
            return;
        }
        // A press in a list whose keys come from the focused field, as a
        // combobox's list box does, leaves the focus in the field: the list
        // is the field's, and its rows are no stops (section 6, rule 3).
        if self
            .focus
            .id()
            .is_some_and(|focused| self.window.in_keys_of(point, focused))
        {
            return;
        }
        let to = self.window.focus_at_in(point, keep).flatten().cloned();
        if to.as_ref().is_some_and(|id| self.takes_no_press_focus(id)) {
            return;
        }
        // While an overlay or a question owns the input, a press on no stop
        // (its backdrop, padding, or a label) keeps the focus where it was,
        // so the user keeps their place in the dialog, as a click on a part
        // of a modal dialog that takes no focus does on the web (the
        // WAI-ARIA dialog pattern). A focus that is gone falls back by
        // section 6, rule 4.
        if modal && to.is_none() && self.focus.id().is_some() {
            return;
        }
        self.focus.wanted = None;
        self.focus.set(to);
        self.focus.ring = false;
        if self.focus.id().is_some() {
            self.focus.release_owed = true;
        }
    }
}

#[cfg(test)]
mod tests;
