//! Tests for what the canvas paints above its GPU shapes, with a painter
//! that records each piece in order and a measure with fixed widths.

use super::*;
use crate::painter::testing::{Painted, Recorder};
use crate::scene::{HoverText, ModuleView, PanelView, PortShape, PortView, WidgetView};
use crate::{Camera, WidgetValue};
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, PointerButton, PointerEvent};

/// The canvas's place in the tests: not at the window's corner, so a
/// missing offset shows.
const BOUNDS: Rect = Rect {
    x: 100.0,
    y: 50.0,
    width: 800.0,
    height: 600.0,
};

/// A widget with a label and a one-sentence description.
fn widget(kind: WidgetViewKind, label: &str) -> WidgetView {
    WidgetView {
        kind,
        x: 0,
        y: 0,
        label: label.into(),
        hover: HoverText {
            description: "What it does.".into(),
            ..HoverText::default()
        },
        hidden: false,
        exposed: false,
    }
}

/// One module titled "Osc" with one port on each side and the widgets
/// given.
fn scene(widgets: Vec<WidgetView>) -> Scene {
    let port = |label: &str| PortView {
        label: label.into(),
        hover: HoverText::default(),
        colour: Color::WHITE,
        shape: PortShape::Round,
        badge: 0,
        follows_transport: None,
    };
    Scene::new(
        vec![ModuleView {
            size: None,
            unknown: false,
            id: 1,
            title: "Osc".into(),
            hover: HoverText::default(),
            accent: Color::BLACK,
            position: Point::new(40.0, 40.0),
            inputs: vec![port("in")],
            outputs: vec![port("out")],
            panel: PanelView {
                width: 6,
                height: 3,
                widgets,
            },
        }],
        Vec::new(),
    )
}

/// What the canvas paints in this state.
fn painted(model: &CanvasModel<'_>, input: &CanvasInput) -> Recorder {
    let mut recorder = Recorder::default();
    input.paint(model, BOUNDS, &mut recorder);
    recorder
}

/// Sends one event with the pointer at `at`, a world point at zoom 1.
fn send(model: &CanvasModel<'_>, input: &mut CanvasInput, event: Event, at: Point) {
    let moment = Moment {
        pointer: Some(Point::new(BOUNDS.x + at.x, BOUNDS.y + at.y)),
        bounds: BOUNDS,
        now: Instant::now(),
    };
    let _ = input.handle(model, &event, moment);
}

/// A key press that types `text`.
fn typed(key: Key, text: &str) -> Event {
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers: Modifiers::NONE,
        repeat: false,
        text: Some(text.into()),
    })
}

/// The canvas held still at zoom 1 with the origin at its corner.
fn still(model: &CanvasModel<'_>, still: Still) -> CanvasInput {
    CanvasInput::still(model, still, Instant::now())
}

/// The kinds of what was painted from `from` on, as one letter each:
/// `B`egin, `Q`uad, `T`ext, `E`nd.
fn kinds(recorder: &Recorder, from: usize) -> String {
    recorder.painted[from..]
        .iter()
        .map(|piece| match piece {
            Painted::Begin(_) => 'B',
            Painted::Quad(_) => 'Q',
            Painted::Text { .. } => 'T',
            Painted::End => 'E',
        })
        .collect()
}

/// Close up, a tile paints its title, its port labels, and its control
/// captions, with no box and no layer, each run clipped to the tile and
/// placed by the canvas's offset in the window.
#[test]
fn a_tile_paints_its_title_ports_and_captions() {
    let scene = scene(vec![widget(WidgetViewKind::Knob { steps: 0 }, "gain")]);
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let recorder = painted(&model, &still(&model, Still::default()));
    let texts = recorder.texts();
    assert_eq!(texts[0], "Osc", "the title is first");
    for text in ["in", "out", "gain"] {
        assert!(texts.contains(&text), "{text} in {texts:?}");
    }
    assert_eq!(kinds(&recorder, 0), "T".repeat(texts.len()));

    let tile = scene.module(1).unwrap().bounds();
    let on_screen = Rect::new(Point::new(BOUNDS.x + tile.x, BOUNDS.y + tile.y), tile.size());
    for piece in &recorder.painted {
        let Painted::Text { run, .. } = piece else {
            panic!("only text");
        };
        assert_eq!(run.clip, on_screen);
        assert!(on_screen.expand(1.0).contains(run.anchor), "{run:?}");
        assert!(!run.wrap);
        assert_eq!(run.vertical, Vertical::Center);
    }
}

/// Zoomed out until the title is too small to read, the canvas paints no
/// text. A little closer, titles show and labels do not.
#[test]
fn text_too_small_to_read_is_not_painted() {
    let scene = scene(vec![widget(WidgetViewKind::Knob { steps: 0 }, "gain")]);
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let at = |zoom| {
        let input = still(
            &model,
            Still {
                camera: Camera {
                    origin: Point::ORIGIN,
                    zoom,
                },
                ..Still::default()
            },
        );
        painted(&model, &input)
            .texts()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert!(at(0.2).is_empty());
    assert_eq!(at(0.7), ["Osc"]);
}

/// A knob with keyboard focus shows its value where its caption was, in
/// the full text colour.
#[test]
fn a_focused_knob_paints_its_value_for_its_caption() {
    let scene = scene(vec![widget(WidgetViewKind::Knob { steps: 0 }, "gain")]);
    let selected = BTreeSet::from([1]);
    let mut values = PanelValues::new();
    values.set(
        1,
        0,
        WidgetValue {
            position: 0.5,
            text: "-6.0 dB".into(),
            ..WidgetValue::default()
        },
    );
    let model = CanvasModel::new(&scene, &selected).values(&values);
    let input = still(
        &model,
        Still {
            focus: Some((1, 0)),
            ..Still::default()
        },
    );
    let recorder = painted(&model, &input);
    let texts = recorder.texts();
    assert!(texts.contains(&"-6.0 dB") && !texts.contains(&"gain"), "{texts:?}");
    let value = recorder
        .painted
        .iter()
        .find_map(|piece| match piece {
            Painted::Text { text, run } if text == "-6.0 dB" => Some(run),
            _ => None,
        })
        .unwrap();
    assert_eq!(value.colour, model.palette.text);
}

/// An open choice list is one layer after all tile text: its box, the
/// highlight under the highlighted row, then each row's text, with a check
/// mark on the chosen one. The rows need full shaping for the mark.
#[test]
fn an_open_choice_list_paints_in_its_own_layer_over_the_tile_text() {
    let scene = scene(vec![widget(
        WidgetViewKind::Select {
            options: vec!["ADSR".into(), "AD".into()],
        },
        "shape",
    )]);
    let selected = BTreeSet::from([1]);
    let model = CanvasModel::new(&scene, &selected);
    let mut input = still(&model, Still::default());
    let tile_text = painted(&model, &input).painted.len();
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    send(
        &model,
        &mut input,
        Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        centre,
    );
    send(
        &model,
        &mut input,
        Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        centre,
    );

    let recorder = painted(&model, &input);
    assert_eq!(kinds(&recorder, 0)[..tile_text], "T".repeat(tile_text));
    assert_eq!(kinds(&recorder, tile_text), "BQQTTE", "box, highlight, text, text");
    assert_eq!(recorder.painted[tile_text], Painted::Begin(BOUNDS));
    let rows: Vec<_> = recorder.painted[tile_text..]
        .iter()
        .filter_map(Painted::as_text)
        .collect();
    assert_eq!(rows, ["✓  ADSR", "    AD"]);
    let Painted::Quad(list) = &recorder.painted[tile_text + 1] else {
        panic!("the box is first in the layer");
    };
    assert_eq!(list.border_width, 1.0);
    assert!(BOUNDS.contains(list.rect.position()), "placed by the canvas's offset");
    let Painted::Text { run, .. } = &recorder.painted[tile_text + 3] else {
        panic!("a row");
    };
    assert_eq!(run.clip, BOUNDS);
}

/// An open typed field is one layer: its box, its text, then the caret
/// after the text as the measure sizes it. Bad text gets a thicker outline
/// and a second box with the reason, after the caret.
#[test]
fn a_typed_field_paints_its_box_text_caret_and_reason_in_order() {
    let scene = scene(vec![widget(WidgetViewKind::Knob { steps: 0 }, "gain")]);
    let selected = BTreeSet::from([1]);
    let model = CanvasModel::new(&scene, &selected)
        .typed_check(|_, _, text| text.parse::<f32>().map(|_| ()).map_err(|_| "Not a number".to_string()));
    let mut input = still(
        &model,
        Still {
            focus: Some((1, 0)),
            ..Still::default()
        },
    );
    let tile_text = painted(&model, &input).painted.len();
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    send(&model, &mut input, typed(Key::Character('5'), "5"), centre);
    send(&model, &mut input, typed(Key::Character('0'), "0"), centre);

    let recorder = painted(&model, &input);
    assert_eq!(kinds(&recorder, tile_text), "BQTQE", "box, text, caret");
    let layer = &recorder.painted[tile_text..];
    let (Painted::Quad(field), Painted::Text { text, run }, Painted::Quad(caret)) = (&layer[1], &layer[2], &layer[3])
    else {
        panic!("box, text, caret");
    };
    assert_eq!(text, "50");
    assert_eq!(field.border_width, 1.0);
    assert_eq!(run.clip, field.rect, "the text is cut to the field");
    // Two characters, each half of the field's text size wide.
    assert_eq!(caret.rect.x, run.anchor.x + FIELD_TEXT_PX);
    assert_eq!(caret.rect.width, 1.0);

    send(&model, &mut input, typed(Key::Character('x'), "x"), centre);
    let recorder = painted(&model, &input);
    assert_eq!(kinds(&recorder, tile_text), "BQTQQTE", "then the reason's box and text");
    let layer = &recorder.painted[tile_text..];
    let (Painted::Quad(field), Painted::Quad(reason)) = (&layer[1], &layer[4]) else {
        panic!("the field and the reason's box");
    };
    assert_eq!(field.border_width, 2.0);
    assert_eq!(field.border, model.palette.warning);
    assert_eq!(layer[5].as_text(), Some("Not a number"));
    assert!(reason.rect.y > field.rect.y, "the reason is under the field");
}

/// A field opened with its value selected paints the selection behind the
/// text and no caret.
#[test]
fn a_selected_field_paints_its_selection_under_the_text() {
    let scene = scene(vec![widget(WidgetViewKind::Knob { steps: 0 }, "gain")]);
    let selected = BTreeSet::from([1]);
    let mut values = PanelValues::new();
    values.set(
        1,
        0,
        WidgetValue {
            position: 0.5,
            text: "1.00".into(),
            ..WidgetValue::default()
        },
    );
    let model = CanvasModel::new(&scene, &selected).values(&values);
    let mut input = still(
        &model,
        Still {
            focus: Some((1, 0)),
            ..Still::default()
        },
    );
    let tile_text = painted(&model, &input).painted.len();
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let enter = Event::Key(KeyEvent::Pressed {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        repeat: false,
        text: None,
    });
    send(&model, &mut input, enter, centre);

    let recorder = painted(&model, &input);
    assert_eq!(kinds(&recorder, tile_text), "BQQTE", "box, selection, text");
    let layer = &recorder.painted[tile_text..];
    let (Painted::Quad(selection), Painted::Text { text, run }) = (&layer[2], &layer[3]) else {
        panic!("selection, then text");
    };
    assert_eq!(text, "1.00");
    // Four characters at half the text size, and one pixel on each side.
    assert_eq!(selection.rect.width, 2.0 * FIELD_TEXT_PX + 2.0);
    assert_eq!(selection.rect.x, run.anchor.x - 1.0);
}

/// Hover text is one layer: its box, then the label, then the description,
/// each wrapped to the box and anchored at its top, one under the other.
#[test]
fn hover_text_paints_its_box_then_its_lines_top_down() {
    let scene = scene(vec![widget(WidgetViewKind::Knob { steps: 0 }, "gain")]);
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let input = still(
        &model,
        Still {
            pointer: Some(centre),
            hover_text: true,
            ..Still::default()
        },
    );
    let recorder = painted(&model, &input);
    let start = recorder
        .painted
        .iter()
        .position(|piece| matches!(piece, Painted::Begin(_)))
        .expect("hover text opens a layer");
    assert_eq!(kinds(&recorder, start), "BQTTE");
    let layer = &recorder.painted[start..];
    let (
        Painted::Quad(frame),
        Painted::Text {
            text: label,
            run: first,
        },
        Painted::Text {
            text: about,
            run: second,
        },
    ) = (&layer[1], &layer[2], &layer[3])
    else {
        panic!("box, label, description");
    };
    assert_eq!((label.as_str(), about.as_str()), ("gain", "What it does."));
    assert!(first.wrap && second.wrap);
    assert_eq!(first.vertical, Vertical::Top);
    assert_eq!(first.style.weight, Weight::Regular);
    assert_eq!(first.anchor.x, frame.rect.x + HOVER_PADDING);
    assert_eq!(first.anchor.y, frame.rect.y + HOVER_PADDING);
    assert!(second.anchor.y >= first.anchor.y + first.bounds.height);
    assert!(
        frame.rect.x >= BOUNDS.x && frame.rect.y >= BOUNDS.y,
        "inside the canvas"
    );
}

/// With a choice list open, hover text is not painted.
#[test]
fn an_open_list_hides_hover_text() {
    let scene = scene(vec![widget(
        WidgetViewKind::Select {
            options: vec!["ADSR".into(), "AD".into()],
        },
        "shape",
    )]);
    let selected = BTreeSet::from([1]);
    let model = CanvasModel::new(&scene, &selected);
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let mut input = still(
        &model,
        Still {
            pointer: Some(centre),
            hover_text: true,
            ..Still::default()
        },
    );
    send(
        &model,
        &mut input,
        Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        centre,
    );
    send(
        &model,
        &mut input,
        Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        centre,
    );
    let recorder = painted(&model, &input);
    let layers = recorder
        .painted
        .iter()
        .filter(|piece| matches!(piece, Painted::Begin(_)))
        .count();
    assert_eq!(layers, 1, "only the list");
}
