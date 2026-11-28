//! The patch canvas as a custom element of the runtime's (ui-layer.md,
//! section 9; plan for leaving Iced, step P4).
//!
//! The canvas is a panel of its own, `canvas`, in a slot of the window's
//! root panel (`chrome.rs`): one custom element, whose UI ID is the
//! panel's, so its parts keep their `canvas/…` UI IDs. The runtime keeps
//! its state ([`CanvasElement`]) by that UI ID, the same at every level of
//! a nested patch, so the camera and the GPU data last through a level
//! change.
//!
//! Each call builds the canvas's model from the app ([`App::canvas_model`])
//! and hands the event to [`CanvasInput`], which knows nothing of the host.
//! Two things Iced gave the canvas for free are the app's to give now, and
//! both run on the element's own deadline, so they need no host event:
//!
//! - **A sync after every update.** Iced drew after every update, and the
//!   canvas did its sync with the app on that redraw: framing a new patch,
//!   a focus or centre request, the lights on screen, and the rest. The app
//!   counts its updates ([`App::updates`]); while the count moved since the
//!   element last synced, its deadline is due at once, and its tick runs
//!   the canvas's redraw. What that sends runs in the same turn and counts
//!   again, so it syncs once more, and stops when a sync sends nothing.
//!   The first sync after an update asks for a frame, as Iced drew after
//!   each update: the canvas's live values change with no panel's.
//! - **The window losing the keys.** The runtime tells only its routing
//!   target. The app counts each loss ([`App::unfocused`]), and the next
//!   sync tells the canvas first, so a held key or an open list lets go.
//! - **A press elsewhere in the window.** Iced gave the canvas every press,
//!   and one outside it took the keys from the canvas. The element hears
//!   only its own, so the app counts the others ([`App::pressed_elsewhere`])
//!   and the next sync replays one, with the pointer outside.
//!
//! - **A shortcut the focused part left.** Iced gave the canvas every
//!   key; the element hears keys only while it or nothing has the focus.
//!   The app queues a shortcut nothing took ([`App::canvas_keys`]), and
//!   the next sync replays it with the pointer where it is.
//!
//! All four are candidates for host events if a second app needs them
//! ("after update", "window unfocused", "pressed elsewhere", "keys left").

use crate::{App, Message};
use loom_canvas::{CanvasInput, CanvasModel, CanvasRenderer, FrameCache, Reaction};
use loom_ui::{Custom, CustomEvent, CustomGpu, CustomKind, Gpu, Layer, Moment, Out, Pass, Used};
use loom_ui_core::{Cursor, Event, Point, PointerEvent, UiId};
use std::time::Instant;

/// The shortcut keys the canvas hears on its next sync, numbered so the
/// canvas replays each once. Only the last few are kept: the canvas syncs
/// in the turn of the key that queued one (section 9, rule 4), so more
/// never wait.
#[derive(Debug, Default)]
pub(crate) struct CanvasKeys {
    /// The number of the last key queued.
    last: u64,
    /// The latest keys, with their numbers, oldest first.
    recent: std::collections::VecDeque<(u64, loom_ui_core::KeyEvent)>,
}

impl CanvasKeys {
    /// The most keys kept.
    const KEPT: usize = 8;

    /// Queues `key`.
    pub(crate) fn push(&mut self, key: loom_ui_core::KeyEvent) {
        self.last += 1;
        if self.recent.len() == Self::KEPT {
            self.recent.pop_front();
        }
        self.recent.push_back((self.last, key));
    }

    /// The keys queued after number `seen`, in order.
    fn after(&self, seen: u64) -> impl Iterator<Item = &loom_ui_core::KeyEvent> {
        self.recent.iter().filter(move |(n, _)| *n > seen).map(|(_, key)| key)
    }
}

/// The canvas's kind of custom element.
pub(crate) const CANVAS: CustomKind = CustomKind::new("canvas");

/// The canvas's panel, and the UI ID of the element that fills it.
pub(crate) fn canvas_id() -> UiId {
    UiId::panel("canvas")
}

/// The canvas's state, which the runtime keeps by [`canvas_id`].
pub(crate) struct CanvasElement {
    /// Its input state: the camera, focus, gestures, and timing.
    input: CanvasInput,
    /// The drawing data built from the scene, rebuilt only when the scene
    /// changes (ADR-0010).
    cache: FrameCache,
    /// The canvas's renderer, made at its first prepare.
    renderer: Option<CanvasRenderer>,
    /// A second renderer for the cable dragged above the panels.
    drag: Option<CanvasRenderer>,
    /// The app's update count at its last sync, `None` before the first.
    synced: Option<u64>,
    /// The app's count of focus losses the canvas has been told of.
    unfocused: u64,
    /// The app's count of presses outside the canvas it has been told of.
    pressed_elsewhere: u64,
    /// The number of the last queued shortcut key it has heard.
    keys_seen: u64,
    /// The moment of the last call, from which a sync is due at once.
    last_now: Option<Instant>,
    /// The modifier keys the canvas was last told of: the host gives the
    /// held keys with each moment, and the canvas hears a change as Iced
    /// sent it, as an event of its own.
    modifiers: loom_ui_core::Modifiers,
    /// The moment of its last tick: a deadline of its own at or before it
    /// was ticked for already. The edit group's deadline is the app's and
    /// moves only in the update after the tick (`EditGroupDue`).
    ticked: Option<Instant>,
}

impl CanvasElement {
    /// A new canvas, or one that starts as `still` describes, for a still
    /// image of the window.
    pub(crate) fn new(app: &App) -> Self {
        let input = match app.still {
            Some(still) => CanvasInput::still(&app.canvas_model(), still, app.clock.now()),
            None => CanvasInput::default(),
        };
        Self {
            input,
            cache: FrameCache::default(),
            renderer: None,
            drag: None,
            synced: None,
            unfocused: app.unfocused,
            pressed_elsewhere: app.pressed_elsewhere,
            keys_seen: app.canvas_keys.last,
            last_now: None,
            modifiers: loom_ui_core::Modifiers::NONE,
            ticked: None,
        }
    }

    /// Builds the canvas's frame as the window's prepare does, and drops
    /// it: the `Ui` harness draws no GPU frame, and this keeps the canvas's
    /// render counts as the window keeps them.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn build_frame(&self, app: &App, bounds: loom_ui_core::Rect) {
        let _ = self.input.drawing(&app.canvas_model(), bounds, &self.cache);
    }

    /// The canvas's input state, for what reads its camera, pointer, and
    /// typed field: `locate`, the snapshot, the accessibility tree, and the
    /// `Ui` harness.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn input(&self) -> &CanvasInput {
        &self.input
    }

    /// Syncs the canvas with the app: tells it the window lost the keys,
    /// if it did, then runs its redraw.
    fn sync(&mut self, app: &App, model: &CanvasModel<'_>, moment: loom_canvas::Moment) -> Reaction {
        // What the canvas draws comes from the app: its lights, meters,
        // traces, selection, and more. Iced drew after every update, so
        // the first sync after one asks for a frame too; the runtime
        // composes one for a burst of them, and the frame re-uploads only
        // the data whose generation moved (ADR-0010).
        let mut reaction = Reaction {
            redraw: self.synced != Some(app.updates),
            ..Reaction::default()
        };
        self.synced = Some(app.updates);
        if self.unfocused != app.unfocused {
            self.unfocused = app.unfocused;
            merge(&mut reaction, self.input.handle(model, &Event::WindowUnfocused, moment));
        }
        if self.pressed_elsewhere != app.pressed_elsewhere {
            self.pressed_elsewhere = app.pressed_elsewhere;
            let outside = loom_canvas::Moment {
                pointer: None,
                ..moment
            };
            let press = Event::Pointer(PointerEvent::Pressed(loom_ui_core::PointerButton::Left));
            merge(&mut reaction, self.input.handle(model, &press, outside));
        }
        // The focused part left these keys, so they are the canvas's now,
        // whatever has the focus: a field would have taken its own paste.
        if self.keys_seen != app.canvas_keys.last {
            let left = app.canvas_model().focus_policy(loom_canvas::CanvasFocusPolicy {
                text_field_keys: false,
                ..app.canvas_focus_policy()
            });
            for key in app.canvas_keys.after(self.keys_seen) {
                merge(
                    &mut reaction,
                    self.input.handle(&left, &Event::Key(key.clone()), moment),
                );
            }
            self.keys_seen = app.canvas_keys.last;
        }
        merge(&mut reaction, self.input.handle(model, &Event::Redraw, moment));
        reaction
    }

    /// True while the app changed since the last sync.
    fn behind(&self, app: &App) -> bool {
        self.synced != Some(app.updates)
            || self.unfocused != app.unfocused
            || self.pressed_elsewhere != app.pressed_elsewhere
            || self.keys_seen != app.canvas_keys.last
    }
}

/// Adds `more` to `reaction`.
fn merge(reaction: &mut Reaction, more: Reaction) {
    reaction.events.extend(more.events);
    reaction.taken |= more.taken;
    reaction.redraw |= more.redraw;
}

/// The canvas's moment for a host moment.
fn canvas_moment(moment: &Moment) -> loom_canvas::Moment {
    loom_canvas::Moment {
        pointer: moment.pointer,
        bounds: moment.bounds,
        now: moment.now,
    }
}

impl Custom<App> for CanvasElement {
    fn event(&mut self, app: &App, event: &CustomEvent, moment: &Moment, out: &mut Out<'_, Message>) -> Used {
        let model = app.canvas_model();
        let at = canvas_moment(moment);
        self.last_now = Some(moment.now);
        // The held modifiers first, so a Shift drag is fine from its first
        // move.
        let mut reaction = Reaction::default();
        if moment.modifiers != self.modifiers {
            self.modifiers = moment.modifiers;
            let changed = Event::Key(loom_ui_core::KeyEvent::ModifiersChanged(moment.modifiers));
            reaction = self.input.handle(&model, &changed, at);
        }
        let mut press = false;
        let heard = match event {
            CustomEvent::Pointer(pointer) => {
                press = matches!(pointer, PointerEvent::Pressed(_));
                self.input.handle(&model, &Event::Pointer(*pointer), at)
            }
            // Iced gave the canvas the move that took the pointer off it,
            // with the pointer where it went; it clears its hover there.
            CustomEvent::PointerLeft => self.input.handle(&model, &Event::Pointer(PointerEvent::Moved), at),
            CustomEvent::CaptureLost => self.input.handle(&model, &Event::Pointer(PointerEvent::Lost), at),
            CustomEvent::Key(key) => self.input.handle(&model, &Event::Key(key.clone()), at),
            CustomEvent::Placed => {
                self.ticked = Some(moment.now);
                // The sync that framed a new patch or level ran before the
                // layout that gave the canvas its size, as when opening a
                // sub-patch shows the breadcrumb; Iced drew after that
                // layout, so the framing is redone at the size shown.
                let mut reaction = self.input.refit(&model, at);
                merge(&mut reaction, self.sync(app, &model, at));
                reaction
            }
            CustomEvent::Deadline => {
                self.ticked = Some(moment.now);
                self.sync(app, &model, at)
            }
            // The measuring animation moves the camera and sends nothing.
            CustomEvent::Frame => self.input.animation_frame(&model, at),
            _ => self.input.pass(&model, at),
        };
        merge(&mut reaction, heard);
        for event in reaction.events {
            out.send(Message::Canvas(event));
        }
        if reaction.redraw {
            out.redraw();
        }
        match (reaction.taken, press) {
            (false, _) => Used::No,
            (true, true) => Used::Capture,
            (true, false) => Used::Yes,
        }
    }

    fn cursor(&self, app: &App, moment: &Moment) -> Cursor {
        let over = moment.pointer.is_some_and(|at| moment.bounds.contains(at));
        self.input.cursor(&app.canvas_model(), over)
    }

    fn next_deadline(&self, app: &App) -> Option<Instant> {
        let own = self
            .input
            .next_deadline(&app.canvas_model())
            .filter(|at| self.ticked.is_none_or(|ticked| *at > ticked));
        let sync = self.last_now.filter(|_| self.behind(app));
        [own, sync].into_iter().flatten().min()
    }

    fn animating(&self, app: &App) -> bool {
        self.input.animating(&app.canvas_model())
    }

    fn gpu(&self) -> Option<&dyn CustomGpu<App>> {
        Some(self)
    }

    fn gpu_mut(&mut self) -> Option<&mut dyn CustomGpu<App>> {
        Some(self)
    }
}

impl CustomGpu<App> for CanvasElement {
    fn prepare(&mut self, app: &App, gpu: &Gpu<'_>) {
        let model = app.canvas_model();
        let drawing = self.input.drawing(&model, gpu.bounds, &self.cache);
        let renderer = self
            .renderer
            .get_or_insert_with(|| CanvasRenderer::new(gpu.device, gpu.format));
        let target = loom_ui::paint::Target {
            size: gpu.bounds.size(),
            scale: gpu.target.scale,
            origin: Point::new(gpu.bounds.x, gpu.bounds.y),
            physical: gpu.target.physical,
        };
        renderer.prepare(gpu.device, gpu.queue, &drawing.frame, target);
    }

    fn draw(&self, pass: &mut Pass<'_>) {
        if let Some(renderer) = &self.renderer {
            renderer.draw(pass);
        }
    }

    /// The cable dragged from a port to the inspector or the matrix shows
    /// over the panels it is dragged to (section 9, rule 8).
    fn above(&self, app: &App) -> Option<Layer> {
        app.drag_line().map(|_| Layer::Panels)
    }

    fn prepare_above(&mut self, app: &App, gpu: &Gpu<'_>) {
        let Some(line) = app.drag_line() else { return };
        let renderer = self
            .drag
            .get_or_insert_with(|| CanvasRenderer::new(gpu.device, gpu.format));
        let relative = |point: Point| Point::new(point.x / line.zoom, point.y / line.zoom);
        let cable =
            loom_canvas::Cable::new(relative(line.start), relative(line.at), line.colour, line.colour, false).focused();
        renderer.prepare_cable(gpu.device, gpu.queue, cable, line.zoom, gpu.target);
    }

    fn draw_above(&self, pass: &mut Pass<'_>) {
        if let Some(drag) = &self.drag {
            drag.draw_cable(pass);
        }
    }

    fn gpu_lost(&mut self) {
        self.renderer = None;
        self.drag = None;
    }
}

/// The cable an output drag shows across the window: from its port to the
/// pointer, in window logical pixels, with the canvas's zoom.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DragLine {
    pub(crate) start: Point,
    pub(crate) at: Point,
    pub(crate) colour: loom_ui_core::Color,
    pub(crate) zoom: f32,
}

/// Where the canvas is in the window and how it looks at the patch, read
/// from its element in this turn, or `None` before its first layout.
pub(crate) fn live_view<M>(cx: &loom_ui::Context<'_, M>) -> Option<loom_canvas::CanvasView> {
    let bounds = cx.bounds(&canvas_id())?;
    Some(cx.custom::<CanvasElement>(&canvas_id())?.input.view(bounds))
}

/// Where the canvas is in the window and how it looks at the patch, read
/// from its element between turns, as a driver holds the panels.
pub(crate) fn panel_view(panels: &loom_ui::Panels<'_, App>) -> Option<loom_canvas::CanvasView> {
    let bounds = panels.bounds(&canvas_id())?;
    Some(panels.custom::<CanvasElement>(&canvas_id())?.input.view(bounds))
}

/// What the canvas says the pointer is on, and its open typed field, read
/// from its element between turns, for the control channel's snapshot.
pub(crate) fn panel_pointer(
    app: &App,
    panels: &loom_ui::Panels<'_, App>,
) -> (Option<loom_canvas::CanvasPointer>, Option<loom_canvas::CanvasTyped>) {
    let (Some(bounds), Some(element)) = (
        panels.bounds(&canvas_id()),
        panels.custom::<CanvasElement>(&canvas_id()),
    ) else {
        return (None, None);
    };
    let model = app.canvas_model();
    (
        Some(element.input.pointer(&model)),
        element.input.typed_field(&model, bounds),
    )
}

/// The canvas's open typed field, read from its element in this turn.
pub(crate) fn live_typed<M>(app: &App, cx: &loom_ui::Context<'_, M>) -> Option<loom_canvas::CanvasTyped> {
    let bounds = cx.bounds(&canvas_id())?;
    cx.custom::<CanvasElement>(&canvas_id())?
        .input
        .typed_field(&app.canvas_model(), bounds)
}

impl App {
    /// Adds the canvas's panel: one custom element, with no key of its own,
    /// so it takes the panel's UI ID, which is the same at every level.
    pub(crate) fn canvas_panel(&self, view: &mut loom_ui::View) {
        view.panel_with(canvas_id(), Layer::Panels, &(), |_| {
            loom_ui::Element::custom(CANVAS)
                .width(loom_ui::Length::FILL)
                .height(loom_ui::Length::FILL)
        });
    }

    /// The cable an output drag shows across the window, while one does.
    pub(crate) fn drag_line(&self) -> Option<DragLine> {
        let drag = self.session.gestures.assign_drag()?;
        let at = drag.at?;
        let port = self.session.scene.port(drag.from)?;
        Some(DragLine {
            start: drag.start,
            at,
            colour: port.colour,
            zoom: drag.zoom,
        })
    }

    /// The canvas's model: what it shows now, built from the app's state.
    pub(crate) fn canvas_model(&self) -> CanvasModel<'_> {
        use crate::edits::ArrowGroup;
        use loom_canvas::EditGroupView;
        let mut model = CanvasModel::new(&self.session.scene, &self.session.selection)
            .display_scale(self.window_scale)
            .values(&self.session.values)
            .window_drag_line(self.session.gestures.assign_drag().is_some())
            .traces(&self.session.traces)
            .activity(
                self.session
                    .carrying
                    .iter()
                    .map(|output| (output.module.0, output.port)),
            )
            .frame_key(self.frame_key)
            .level(
                self.session.level_key,
                self.session.cameras.get(&self.session.level).copied(),
            )
            .browser_key(self.browser_key)
            .focus_policy(self.canvas_focus_policy())
            .focus_request(self.a11y.focus_request().0, self.a11y.focus_request().1)
            .edit_group(match self.session.edits.arrow_group() {
                // A screen reader's group waits as a released arrow group
                // does, so the canvas is shown both as an open group: it
                // wakes once at the deadline, and reports the key or
                // pointer press that ends the group sooner. Nothing wakes
                // while no group is open.
                ArrowGroup::Closed => match self.session.edits.reader_group().due() {
                    Some(due) => EditGroupView::Open { due },
                    None => EditGroupView::Closed,
                },
                ArrowGroup::Held => EditGroupView::Held,
                ArrowGroup::Released { due } => EditGroupView::Open { due },
            })
            .matrix_line(self.matrix_line())
            .typed_check(|module, widget, text| self.typed_check(module, widget, text))
            .report_typed(self.a11y.is_active())
            .typed_request(self.a11y.typed_request().0, self.a11y.typed_request().1)
            .overview(self.map_view())
            .centre_request(self.session.outline.centre.0, self.session.outline.centre.1)
            .animate(self.animating)
            .learn(self.session.learn.shown());
        if let Some(still) = self.still {
            model = model.still(still);
        }
        model
    }
}

#[cfg(test)]
mod tests;
