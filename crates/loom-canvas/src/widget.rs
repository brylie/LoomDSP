//! The patch canvas as an Iced widget.
//!
//! This is the only Iced code in the canvas beside the GPU primitive. It
//! turns Iced's events into ours, hands them to [`CanvasInput`], applies the
//! [`Reaction`] to Iced's shell, and draws. The input handling itself is in
//! `crate::input` and knows nothing of Iced.

use crate::CanvasEvent;
use crate::camera::Camera;
use crate::controls::{ControlGeometry, PanelValues};
use crate::gpu::{CanvasFrame, Geometry};
use crate::hover::{HOVER_PADDING, HOVER_TEXT_PX, HoverTarget};
use crate::iced_convert::{ToCore as _, ToIced as _};
use crate::input::*;
use crate::reaction::Reaction;
use crate::scene::{BADGE_TEXT_SIZE, Scene, WidgetViewKind};
use crate::theme::Palette;
use crate::tile_text::{self, Align, LABEL_SIZE, TITLE_SIZE, TextKind};
use crate::trace::{ScopeTraces, TraceGeometry};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{Element, Font, Length};
use iced::{mouse, window};
use loom_ui_core::{Color, Point, Rect, Size};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

mod drag_line;
mod draw;
mod painter;
mod primitive;

pub use drag_line::DragLine;
use painter::{IcedMeasure, IcedPainter};
use primitive::CanvasPrimitive;

/// The patch canvas as an Iced widget. Draws a [`Scene`] and reports
/// gestures as [`CanvasEvent`]s, through the message `on_event` makes.
pub struct PatchCanvas<'a, Message> {
    /// What to show this frame.
    model: CanvasModel<'a>,
    /// Turns what the canvas reports into the app's message.
    on_event: Box<dyn Fn(CanvasEvent) -> Message + 'a>,
}

/// What the widget keeps between frames: the canvas's input state, and the
/// drawing data built from the scene, rebuilt only when the scene changes
/// (ADR-0010).
#[derive(Default)]
struct State {
    input: CanvasInput,
    geometry: RefCell<Option<Arc<Geometry>>>,
    controls: RefCell<Option<Arc<ControlGeometry>>>,
    traces: RefCell<Option<Arc<TraceGeometry>>>,
    lines: LinesCache,
}

impl<'a, Message> PatchCanvas<'a, Message> {
    /// Creates a canvas for a scene and the current selection.
    pub fn new(scene: &'a Scene, selection: &'a BTreeSet<u32>, on_event: impl Fn(CanvasEvent) -> Message + 'a) -> Self {
        Self {
            model: CanvasModel::new(scene, selection),
            on_event: Box::new(on_event),
        }
    }

    /// Starts the canvas with this camera and focus, for one still image.
    /// See [`CanvasModel::still`].
    pub fn still(mut self, still: Still) -> Self {
        self.model = self.model.still(still);
        self
    }

    /// Turns on MIDI learn mode, with what it shows.
    /// See [`CanvasModel::learn`].
    pub fn learn(mut self, learn: Option<&'a LearnView>) -> Self {
        self.model = self.model.learn(learn);
        self
    }

    /// Shows one matrix-selected assignment line without pinning its source.
    /// See [`CanvasModel::matrix_line`].
    pub fn matrix_line(mut self, line: Option<crate::scene::ModulationView>) -> Self {
        self.model = self.model.matrix_line(line);
        self
    }

    /// Says who else in the window has the keyboard.
    /// See [`CanvasModel::focus_policy`].
    pub fn focus_policy(mut self, policy: CanvasFocusPolicy) -> Self {
        self.model = self.model.focus_policy(policy);
        self
    }

    /// Says where the app's arrow-key undo group is (`docs/design/undo.md`).
    /// See [`CanvasModel::arrow_group`].
    pub fn arrow_group(mut self, group: ArrowGroupView) -> Self {
        self.model = self.model.arrow_group(group);
        self
    }

    /// Sets the live values of panel controls and lights.
    /// See [`CanvasModel::values`].
    pub fn values(mut self, values: &'a PanelValues) -> Self {
        self.model = self.model.values(values);
        self
    }

    /// The app draws output drags in its window layer, including the canvas.
    /// See [`CanvasModel::window_drag_line`].
    pub fn window_drag_line(mut self, enabled: bool) -> Self {
        self.model = self.model.window_drag_line(enabled);
        self
    }

    /// Sets the scope traces.
    /// See [`CanvasModel::traces`].
    pub fn traces(mut self, traces: &'a ScopeTraces) -> Self {
        self.model = self.model.traces(traces);
        self
    }

    /// Sets the colours.
    /// See [`CanvasModel::palette`].
    pub fn palette(mut self, palette: Palette) -> Self {
        self.model = self.model.palette(palette);
        self
    }

    /// Frames the whole scene whenever this key changes.
    /// See [`CanvasModel::frame_key`].
    pub fn frame_key(mut self, key: u64) -> Self {
        self.model = self.model.frame_key(key);
        self
    }

    /// Says which level of a nested patch the scene shows, and its camera.
    /// See [`CanvasModel::level`].
    pub fn level(mut self, key: u64, camera: Option<Camera>) -> Self {
        self.model = self.model.level(key, camera);
        self
    }

    /// Opens the module browser at the centre whenever this key changes.
    /// See [`CanvasModel::browser_key`].
    pub fn browser_key(mut self, key: u64) -> Self {
        self.model = self.model.browser_key(key);
        self
    }

    /// Moves the camera continuously in a slow loop, for measuring.
    /// See [`CanvasModel::animate`].
    pub fn animate(mut self, animate: bool) -> Self {
        self.model = self.model.animate(animate);
        self
    }

    /// Sets the check for typed values.
    /// See [`CanvasModel::typed_check`].
    pub fn typed_check(mut self, check: impl Fn(u32, usize, &str) -> Result<(), String> + 'a) -> Self {
        self.model = self.model.typed_check(check);
        self
    }

    /// Sets live port activity without changing the scene's GPU geometry.
    /// See [`CanvasModel::activity`].
    pub fn activity(mut self, outputs: impl IntoIterator<Item = (u32, usize)>) -> Self {
        self.model = self.model.activity(outputs);
        self
    }

    /// Hands a reaction to Iced: the events as messages, the input as
    /// captured, and the redraw as a request for the next frame.
    fn apply(&self, reaction: Reaction, shell: &mut Shell<'_, Message>) {
        for event in reaction.events {
            shell.publish((self.on_event)(event));
        }
        if reaction.taken {
            shell.capture_event();
        }
        if reaction.redraw {
            shell.request_redraw();
        }
    }

    /// Asks Iced to wake the canvas at its next deadline, if it has one.
    ///
    /// Iced forgets a timed request when a later one asks for the next
    /// frame, so this runs after every Iced event, whether or not the event
    /// means anything to the canvas. Without it, an arrow group could stay
    /// open on an idle patch.
    fn schedule(&self, input: &CanvasInput, shell: &mut Shell<'_, Message>) {
        if let Some(deadline) = input.next_deadline(&self.model) {
            shell.request_redraw_at(deadline);
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for PatchCanvas<'_, Message>
where
    Renderer: iced_wgpu::primitive::Renderer + text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let input = match self.model.still {
            Some(still) => CanvasInput::still(&self.model, still, Instant::now()),
            None => CanvasInput::default(),
        };
        tree::State::new(State {
            input,
            ..State::default()
        })
    }

    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &iced::Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        // A redraw carries the time of its frame. Every other event
        // happened now.
        let now = match event {
            iced::Event::Window(window::Event::RedrawRequested(at)) => *at,
            _ => Instant::now(),
        };
        let moment = Moment {
            pointer: cursor.position().map(|point| point.to_core()),
            bounds: layout.bounds().to_core(),
            now,
        };
        // An Iced event with no meaning to the canvas still passes through,
        // so what runs on every event keeps running.
        let reaction = match crate::iced_convert::event_to_core(event) {
            Some(event) => state.input.handle(&self.model, &event, moment),
            None => state.input.pass(&self.model, moment),
        };
        self.apply(reaction, shell);
        self.schedule(&state.input, shell);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let mut view = state.input.view(layout.bounds().to_core());
        operation.custom(None, layout.bounds(), &mut view);
        // What the pointer is on, for the control channel's snapshot.
        let mut pointer = state.input.pointer(&self.model);
        operation.custom(None, layout.bounds(), &mut pointer);
        // The open typed field, for the same snapshot.
        if let Some(mut typed) = state.input.typed() {
            operation.custom(None, layout.bounds(), &mut typed);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &iced::Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        state
            .input
            .cursor(&self.model, cursor.is_over(layout.bounds()))
            .to_iced()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds().to_core();
        let model = &self.model;
        let input = &state.input;
        let camera = input.camera();

        let geometry = {
            let mut cache = state.geometry.borrow_mut();
            match cache.as_ref() {
                Some(g) if g.generation == model.scene.generation() => g.clone(),
                _ => {
                    let built = Arc::new(Geometry::build(model.scene, &model.palette));
                    *cache = Some(built.clone());
                    built
                }
            }
        };

        // Controls follow the titles: drawn as shapes once titles show, and
        // not at all when zoomed further out (panels.md, Level of detail).
        let empty = PanelValues::default();
        let values = model.values.unwrap_or(&empty);
        let controls = (detail(camera.zoom) != Detail::Far).then(|| {
            let key = (model.scene.generation(), values.generation());
            let mut cache = state.controls.borrow_mut();
            match cache.as_ref() {
                Some(c) if c.key == key => c.clone(),
                _ => {
                    let built = Arc::new(ControlGeometry::build(model.scene, values, &model.palette));
                    *cache = Some(built.clone());
                    built
                }
            }
        });

        // Scope traces: full close up, a flat line at the middle zoom, and
        // not drawn far out (panels.md, Drawing the scope trace).
        let traces = model
            .traces
            .filter(|_| detail(camera.zoom) != Detail::Far)
            .map(|traces| {
                let full = detail(camera.zoom) == Detail::Close;
                let key = (model.scene.generation(), traces.generation(), full);
                let mut cache = state.traces.borrow_mut();
                match cache.as_ref() {
                    Some(t) if t.key == key => t.clone(),
                    _ => {
                        let built = Arc::new(TraceGeometry::build(model.scene, traces, full, &model.palette));
                        *cache = Some(built.clone());
                        built
                    }
                }
            });

        // Text too small to draw leaves a bar in its place.
        let bars = match detail(camera.zoom) {
            Detail::Close => geometry.bars(false, false),
            Detail::Middle => geometry.bars(true, false),
            Detail::Far => geometry.bars(true, true),
        };
        // A box's text bar draws over the box, and only at middle zoom.
        let value_bars = geometry.value_bars(detail(camera.zoom) == Detail::Middle);

        renderer.fill_quad(
            renderer::Quad {
                bounds: bounds.to_iced(),
                ..renderer::Quad::default()
            },
            model.palette.background.to_iced(),
        );
        renderer.draw_primitive(
            bounds.to_iced(),
            CanvasPrimitive(CanvasFrame {
                geometry,
                controls,
                traces,
                overlay: model.overlay_in(input, &state.lines, camera.visible(bounds.size())),
                bars,
                value_bars,
                camera,
                cable_dim: CABLE_OPACITY,
            }),
        );
        let measure = IcedMeasure::new(&*renderer);
        model.paint(input, values, bounds, &measure, &mut IcedPainter::new(renderer));
    }
}

impl<'a, Message, Theme, Renderer> From<PatchCanvas<'a, Message>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Renderer: iced_wgpu::primitive::Renderer + text::Renderer<Font = Font> + 'a,
    Theme: 'a,
{
    fn from(canvas: PatchCanvas<'a, Message>) -> Self {
        Element::new(canvas)
    }
}

#[cfg(test)]
mod tests;
