//! Tests for a frame's plan: the painting cut at each custom element that
//! draws with the GPU, in paint order (ui-layer.md, section 9, rule 4).

use crate::app::{App, Context, WindowSpec};
use crate::custom::{Custom, CustomGpu, CustomKind, FrameStep, Gpu, Pass};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::WindowState;
use crate::headless::Harness;
use crate::layout::tests::STYLE;
use crate::paint::{ListPainter, PaintList, Quad, TextPainter, Typesetter};
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use loom_ui_core::{Color, Rect, Size, UiId};
use std::cell::RefCell;
use std::time::Instant;

/// A pad: an element that draws with the GPU and paints a mark of its own.
const PAD: CustomKind = CustomKind::new("pad");

/// A meter: an element that only paints.
const METER: CustomKind = CustomKind::new("meter");

/// The panel's background.
const BACKGROUND: Color = Color::rgb(0.1, 0.1, 0.1);

/// What the pad paints over its GPU drawing.
const MARK: Color = Color::rgb(0.9, 0.2, 0.2);

/// The overlay's box.
const OVERLAY: Color = Color::rgb(0.2, 0.2, 0.9);

/// A pad's state.
struct PadState;

impl Custom<Pads> for PadState {
    fn paint(&self, _app: &Pads, bounds: Rect, painter: &mut dyn TextPainter) {
        painter.quad(Quad {
            rect: bounds,
            fill: MARK,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
    }

    fn gpu(&self) -> Option<&dyn CustomGpu<Pads>> {
        Some(self)
    }

    fn gpu_mut(&mut self) -> Option<&mut dyn CustomGpu<Pads>> {
        Some(self)
    }
}

impl CustomGpu<Pads> for PadState {
    fn prepare(&mut self, _app: &Pads, _gpu: &Gpu<'_>) {}

    fn draw(&self, _pass: &mut Pass<'_>) {}

    fn above(&self, app: &Pads) -> Option<Layer> {
        app.above.then_some(Layer::Panels)
    }
}

/// A meter's state, which only paints.
struct MeterState;

impl Custom<Pads> for MeterState {}

/// An app with a pad and a meter between two labels on a background, a
/// second pad clipped away, and an overlay.
struct Pads {
    /// True when the pad draws above the Panels layer too.
    above: bool,
}

impl App for Pads {
    type Message = ();

    fn update(&mut self, _message: (), _cx: &Context<'_, ()>) -> Effects<()> {
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &(), |_| {
            let pad = |key| {
                Element::custom(PAD)
                    .key(key)
                    .width(Length::Fixed(100.0))
                    .min_width(100.0)
                    .height(Length::Fixed(40.0))
            };
            Element::column([
                Element::text("before", STYLE),
                pad("pad"),
                Element::custom(METER)
                    .key("meter")
                    .width(Length::Fixed(50.0))
                    .height(Length::Fixed(10.0)),
                Element::text("after", STYLE),
                // Scrolled away: nothing of it shows.
                Element::row([pad("hidden")]).width(Length::Fixed(0.0)).clip(),
            ])
            .background(BACKGROUND)
        });
        view.panel_with(UiId::panel("dialog"), Layer::Overlays, &(), |_| {
            Element::space(Length::Fixed(30.0), Length::Fixed(30.0)).background(OVERLAY)
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        match kind {
            PAD => Some(Box::new(PadState)),
            METER => Some(Box::new(MeterState)),
            _ => None,
        }
    }
}

/// The pad's UI ID.
fn pad() -> UiId {
    UiId::panel("main").with("pad")
}

/// Paints the Panels and Overlays layers of an app into one list, as a
/// frame does, and gives the list and its plan.
fn plan(above: bool) -> (PaintList, Vec<FrameStep>) {
    let text = RefCell::new(Typesetter::new());
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(Pads { above }, NoTarget, window, Instant::now());
    harness.lay_out(&mut *text.borrow_mut());
    let mut list = PaintList::new(Rect::new(loom_ui_core::Point::ORIGIN, Size::new(400.0, 300.0)));
    let (_, _, mut panels) = harness.parts();
    for layer in [Layer::Panels, Layer::Overlays] {
        panels.paint(
            layer,
            &mut ListPainter::new(&text, &mut list),
            &crate::draw::tests::PANEL,
        );
    }
    let steps = panels.frame_plan(&list).steps().to_vec();
    (list, steps)
}

/// The layer of `list` that holds the box filled with `fill`.
fn layer_of(list: &PaintList, fill: Color) -> usize {
    list.layer_of_fill(fill).expect("the box was painted")
}

/// The pad's GPU drawing goes after its parent's background and before
/// its own painting and what is painted after it: one cut, at its layer.
/// The meter, which only paints, makes none.
#[test]
fn the_painting_is_cut_at_the_gpu_element() {
    let (list, steps) = plan(false);
    let FrameStep::Custom { id, bounds, clip } = &steps[1] else {
        panic!("the second step is the pad: {steps:?}");
    };
    assert_eq!(*id, pad());
    assert_eq!(bounds.size(), Size::new(100.0, 40.0));
    assert_eq!(clip, bounds);
    let (FrameStep::Paint(before), FrameStep::Paint(after)) = (&steps[0], &steps[2]) else {
        panic!("paint, the pad, paint: {steps:?}");
    };
    assert_eq!(steps.len(), 3, "one cut only: {steps:?}");
    assert!(before.contains(&layer_of(&list, BACKGROUND)));
    assert!(after.contains(&layer_of(&list, MARK)));
    assert!(after.contains(&layer_of(&list, OVERLAY)));
    assert_eq!(before.end, after.start);
    assert_eq!(after.end, list.layer_count());
}

/// An element with nothing showing gets no step.
#[test]
fn an_element_clipped_away_gets_no_step() {
    let (_, steps) = plan(false);
    let hidden = UiId::panel("main").with("hidden");
    assert!(
        steps
            .iter()
            .all(|step| !matches!(step, FrameStep::Custom { id, .. } if *id == hidden))
    );
}

/// What an element draws above the Panels layer goes after that layer's
/// painting and before the Overlays layer's.
#[test]
fn drawing_above_goes_after_its_layer() {
    let (list, steps) = plan(true);
    let above = steps
        .iter()
        .position(|step| matches!(step, FrameStep::Above { id, layer: Layer::Panels } if *id == pad()))
        .expect("an Above step");
    let FrameStep::Paint(before) = &steps[above - 1] else {
        panic!("painting before it: {steps:?}");
    };
    // Every element that asks draws above, the pad scrolled away too: that
    // drawing is cut to the window, not to its bounds.
    let Some(FrameStep::Paint(after)) = steps[above..].iter().find(|step| matches!(step, FrameStep::Paint(_))) else {
        panic!("painting after it: {steps:?}");
    };
    assert!(before.contains(&layer_of(&list, MARK)));
    assert!(after.contains(&layer_of(&list, OVERLAY)));
}
