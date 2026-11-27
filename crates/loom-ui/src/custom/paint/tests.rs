//! Tests for painting custom elements: in paint order, cut to their bounds
//! and every clip around them, read from the app, and in a still picture
//! (ui-layer.md, section 9, rule 4).

use crate::app::{App, Context, WindowSpec};
use crate::custom::{Custom, CustomKind};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::WindowState;
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::paint::testing::{Painted, Recorder};
use crate::paint::{PanelStyle, Quad, TextPainter};
use crate::routing::NoTarget;
use crate::still::Still;
use crate::view::{Layer, View};
use loom_ui_core::{Color, Rect, Size, UiId};
use std::time::Instant;

/// The meter's kind.
const METER: CustomKind = CustomKind::new("meter");

/// The colour of the panel's background.
const BACKGROUND: Color = Color::rgb(0.1, 0.1, 0.1);

/// The colour of the meter's bar.
const BAR: Color = Color::rgb(0.2, 0.8, 0.2);

/// The meter's state: it keeps nothing, and paints its level from the app.
struct MeterState;

impl Custom<Meters> for MeterState {
    fn paint(&self, app: &Meters, bounds: Rect, painter: &mut dyn TextPainter) {
        painter.quad(Quad {
            rect: Rect::new(bounds.position(), Size::new(bounds.width * app.level, bounds.height)),
            fill: BAR,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
    }
}

/// An app with a meter between two labels, on a panel background, inside
/// a box that clips it to its first 60 pixels across and is taller than
/// it.
struct Meters {
    level: f32,
}

impl App for Meters {
    type Message = ();

    fn update(&mut self, _message: (), _cx: &Context<'_, ()>) -> Effects<()> {
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &(), |_| {
            Element::column([
                Element::text("before", STYLE).key("before"),
                Element::row([Element::custom(METER)
                    .key("meter")
                    .width(Length::Fixed(100.0))
                    .min_width(100.0)
                    .height(Length::Fixed(20.0))])
                .width(Length::Fixed(60.0))
                .height(Length::Fixed(40.0))
                .clip(),
                Element::text("after", STYLE).key("after"),
            ])
            .background(BACKGROUND)
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == METER).then(|| Box::new(MeterState) as Box<dyn Custom<Self>>)
    }
}

/// The panel style, with its colours told apart.
fn style() -> PanelStyle {
    crate::draw::tests::PANEL
}

/// What painting the Panels layer of `harness` records.
fn paint(harness: &mut Harness<Meters, NoTarget>) -> Recorder {
    let mut recorder = Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(Layer::Panels, &mut recorder, &style());
    recorder
}

/// A harness for a meter at `level`, laid out in a 400 by 200 window.
fn harness(level: f32) -> Harness<Meters, NoTarget> {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Meters { level }, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// Where in `recorder` a box filled with `fill` was painted, and its box.
fn quad_of(recorder: &Recorder, fill: Color) -> (usize, Rect) {
    recorder
        .painted
        .iter()
        .enumerate()
        .find_map(|(at, piece)| match piece {
            Painted::Quad(quad) if quad.fill == fill => Some((at, quad.rect)),
            _ => None,
        })
        .expect("the box was painted")
}

/// Where in `recorder` the text `text` was painted.
fn text_at(recorder: &Recorder, text: &str) -> usize {
    recorder
        .painted
        .iter()
        .position(|piece| piece.as_text() == Some(text))
        .expect("the text was painted")
}

/// The meter paints in paint order: over its parent's background and the
/// label before it, under the label after it, reading the app's level.
#[test]
fn a_custom_element_paints_in_paint_order() {
    let mut harness = harness(0.5);
    let recorder = paint(&mut harness);
    let (background, _) = quad_of(&recorder, BACKGROUND);
    let (bar, rect) = quad_of(&recorder, BAR);
    assert!(background < text_at(&recorder, "before"));
    assert!(text_at(&recorder, "before") < bar);
    assert!(bar < text_at(&recorder, "after"));
    assert_eq!(rect.width, 50.0, "half its width, from the app's level");
}

/// The meter paints inside a layer cut to its bounds and to the clip of
/// the box around it, which is narrower than the meter.
#[test]
fn a_custom_element_paints_under_its_clip() {
    let mut harness = harness(1.0);
    let recorder = paint(&mut harness);
    let (bar, _) = quad_of(&recorder, BAR);
    let clip = recorder.painted[..bar]
        .iter()
        .rev()
        .find_map(|piece| match piece {
            Painted::Begin(clip) => Some(*clip),
            _ => None,
        })
        .expect("a layer opened before the bar");
    let meter = harness
        .runtime()
        .custom_bounds(&UiId::panel("main").with("meter"))
        .expect("laid out");
    assert_eq!(clip, Rect::new(meter.position(), Size::new(60.0, 20.0)));
    assert!(matches!(recorder.painted[bar + 1], Painted::End));
}

/// A still picture of the window, with no runtime, paints the meter from
/// a state the app makes for it, so a capture shows it; the plain picture
/// paints none.
#[test]
fn a_still_picture_paints_custom_elements() {
    let app = Meters { level: 0.25 };
    let mut view = View::new();
    app.view(&mut view);
    let mut text = Mono::default();
    let still = Still::new(&view, Size::new(400.0, 200.0), &mut text);
    let mut recorder = Recorder::default();
    still.paint_with(&app, Layer::Panels, &mut recorder, &style());
    assert_eq!(quad_of(&recorder, BAR).1.width, 25.0);
    let mut plain = Recorder::default();
    still.paint(Layer::Panels, &mut plain, &style());
    assert!(
        plain
            .painted
            .iter()
            .all(|piece| !matches!(piece, Painted::Quad(quad) if quad.fill == BAR))
    );
}
