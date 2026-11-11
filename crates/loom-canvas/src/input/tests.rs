use super::focus::{FocusMode, FocusState, RangePart};
use super::panel_input::{focusable, next_focus};
use super::*;
use crate::WidgetValue;
use crate::scene::{CableView, ModuleView, PanelView, PortShape, PortView, WidgetView};

/// Shift alone.
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};
/// Control alone.
const CTRL: Modifiers = Modifiers {
    control: true,
    ..Modifiers::NONE
};
/// Alt alone.
const ALT: Modifiers = Modifiers {
    alt: true,
    ..Modifiers::NONE
};
/// The system key alone: Command on a Mac.
const LOGO: Modifiers = Modifiers {
    meta: true,
    ..Modifiers::NONE
};

/// The key that types the one character in `text`.
fn key_of(text: &str) -> Key {
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(only), None) => Key::Character(only),
        _ => Key::Other,
    }
}

/// The shortcut key of the platform the tests run on, alone.
fn command() -> Modifiers {
    if LOGO.shortcut() { LOGO } else { CTRL }
}

fn module(id: u32, x: f32) -> ModuleView {
    let port = || PortView {
        label: "p".into(),
        hover: crate::scene::HoverText::default(),
        colour: Color::WHITE,
        shape: PortShape::Round,
        badge: 0,
        follows_transport: None,
    };
    ModuleView {
        size: None,
        unknown: false,
        id,
        title: String::new(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::new(x, 0.0),
        inputs: vec![port()],
        outputs: vec![port()],
        panel: PanelView::default(),
    }
}

fn widget(kind: WidgetViewKind, x: u8) -> WidgetView {
    WidgetView {
        kind,
        x,
        y: 0,
        label: String::new(),
        hover: crate::scene::HoverText::default(),
        hidden: false,
        exposed: false,
    }
}

/// A knob, a light, and a toggle.
fn panel() -> PanelView {
    PanelView {
        width: 9,
        height: 3,
        widgets: vec![
            widget(WidgetViewKind::Knob { steps: 0 }, 0),
            widget(WidgetViewKind::Light, 3),
            widget(WidgetViewKind::Toggle, 6),
        ],
    }
}

/// Tab skips a control its module's mode hides (ADR-0030).
#[test]
fn tab_skips_hidden_controls() {
    let mut panel = panel();
    panel.widgets[2].hidden = true;
    assert_eq!(next_focus(&panel, None, false), Some(0));
    assert_eq!(next_focus(&panel, Some(0), false), Some(0), "only the knob is left");
    assert_eq!(focusable(&panel), [0], "Tab within a module walks the same list");
}

#[test]
fn tab_moves_focus_over_controls_and_skips_lights() {
    let panel = panel();
    assert_eq!(next_focus(&panel, None, false), Some(0));
    assert_eq!(next_focus(&panel, Some(0), false), Some(2));
    assert_eq!(next_focus(&panel, Some(2), false), Some(0));
    assert_eq!(next_focus(&panel, Some(0), true), Some(2));
    assert_eq!(next_focus(&panel, None, true), Some(2));
    assert_eq!(next_focus(&PanelView::default(), None, false), None);
}

#[test]
fn detail_follows_the_text_thresholds() {
    assert_eq!(detail(1.0), Detail::Close);
    // Port labels hidden, titles shown.
    assert_eq!(detail(0.7), Detail::Middle);
    assert_eq!(detail(0.3), Detail::Far);
    // Framing stops at the first zoom that still draws labels.
    assert_eq!(detail(READABLE_ZOOM), Detail::Close);
    assert_eq!(detail(READABLE_ZOOM * 0.9), Detail::Middle);
}

#[test]
fn only_lights_on_screen_are_reported() {
    let mut lit = module(1, 0.0);
    lit.panel = panel();
    let far_away = ModuleView {
        panel: panel(),
        ..module(2, 5_000.0)
    };
    let plain = module(3, 200.0);
    let scene = Scene::new(vec![lit, far_away, plain], Vec::new());
    let camera = Camera {
        origin: Point::new(-50.0, -50.0),
        zoom: 1.0,
    };
    let size = Size::new(800.0, 600.0);
    assert_eq!(visible_lights(&scene, &camera, size), vec![1]);
    // Zoomed far out, lights are not drawn, so none need reading.
    let far = Camera { zoom: 0.3, ..camera };
    assert!(visible_lights(&scene, &far, size).is_empty());
}

#[test]
fn a_control_focused_by_keyboard_has_a_ring_clear_of_its_caption() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let unfocused = canvas
        .overlay(&CanvasInput::default(), &LinesCache::default())
        .shapes
        .len();
    let tabbed = CanvasInput {
        focus: FocusState::at(1, 0, true, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let overlay = canvas.overlay(&tabbed, &LinesCache::default());
    assert_eq!(overlay.shapes.len(), unfocused + 1);
    // The ring stays above the caption, where the value shows.
    let ring = overlay.shapes.last().unwrap().rect();
    let module = &scene.modules()[0];
    let mut caption = None;
    tile_text::for_each(module, |item| {
        if item.kind == TextKind::Caption(0) {
            caption = Some(item.bounds());
        }
    });
    let caption = caption.unwrap();
    assert!(
        ring[1] + ring[3] <= caption.y,
        "ring {ring:?} clear of caption {caption:?}"
    );
    // Focus from a click or a drag shows no ring (#92).
    let clicked = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.overlay(&clicked, &LinesCache::default()).shapes.len(), unfocused);
}

#[test]
fn hover_text_is_about_the_port_or_control_under_the_pointer() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let port = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let on = |hover| CanvasInput {
        hover: Some(hover),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.hover_target(&on(Hit::Port(port))), Some(HoverTarget::Port(port)));
    let knob = Hit::Widget { module: 1, widget: 0 };
    assert_eq!(canvas.hover_target(&on(knob)), Some(HoverTarget::Control(1, 0)));
    assert_eq!(canvas.hover_target(&on(Hit::Module(1))), None);
    assert_eq!(canvas.hover_target(&on(Hit::Nothing)), None);
    // On a port's label: the port's hover text (#92).
    let mut label = None;
    tile_text::for_each(&scene.modules()[0], |item| {
        if item.kind == TextKind::Port(port) {
            label = Some(item.bounds().center());
        }
    });
    let on_label = CanvasInput {
        cursor: label,
        ..on(Hit::Module(1))
    };
    assert_eq!(canvas.hover_target(&on_label), Some(HoverTarget::Port(port)));
}

/// Resting on a tile's header shows the module's hover text: its family
/// and description (ADR-0029). The body below the header does not.
#[test]
fn the_header_hovers_with_the_module_family() {
    let mut m = module(1, 0.0);
    m.hover = HoverText {
        description: "Sources. A test module.".into(),
        name: Some("test.module".into()),
        note: None,
    };
    let tile = m.bounds();
    let scene = Scene::new(vec![m], Vec::new());
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let at = |y: f32| CanvasInput {
        hover: Some(Hit::Module(1)),
        cursor: Some(Point::new(tile.center_x(), y)),
        ..CanvasInput::default()
    };
    assert_eq!(
        canvas.hover_target(&at(tile.y + HEADER * 0.5)),
        Some(HoverTarget::Title(1))
    );
    assert_eq!(canvas.hover_target(&at(tile.y + HEADER + 30.0)), None);
    let (_, hover, value, anchor) = canvas.hover_content(HoverTarget::Title(1)).unwrap();
    assert!(hover.description.starts_with("Sources."));
    assert_eq!(value, None);
    assert_eq!(anchor.height, HEADER);
}

/// A port label drawn over another tile's header hovers as the port,
/// not as that tile's title, because text is drawn above every tile.
#[test]
fn a_port_label_over_a_header_hovers_as_the_port() {
    let port = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let mut below = module(1, 0.0);
    below.panel = panel();
    let mut label = None;
    tile_text::for_each(&below, |item| {
        if item.kind == TextKind::Port(port) {
            label = Some(item.bounds().center());
        }
    });
    let label = label.expect("a port label");
    let mut cover = module(2, 0.0);
    cover.inputs.clear();
    cover.outputs.clear();
    cover.position = Point::new(label.x - 40.0, label.y - HEADER * 0.5);
    let scene = Scene::new(vec![below, cover], Vec::new());
    let header = scene.module(2).unwrap().bounds();
    assert!(label.y < header.y + HEADER, "the label is in the cover's header");
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let state = CanvasInput {
        hover: Some(Hit::Module(2)),
        cursor: Some(label),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.hover_target(&state), Some(HoverTarget::Port(port)));
}

/// A light with a description hovers like a control, and its hover
/// text has the value line the app wrote. A light with no description
/// shows none (port-and-parameter-text.md, Display values).
#[test]
fn a_light_shows_its_description_and_value() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let plain = m.clone();
    m.panel.widgets[1].hover = HoverText::new("Gate", "Lit while a key is held.", "gate");
    let mut meter = module(2, 600.0);
    meter.panel = PanelView {
        width: 3,
        height: 6,
        widgets: vec![widget(
            WidgetViewKind::Meter {
                zero: 0.0,
                clips: false,
            },
            0,
        )],
    };
    meter.panel.widgets[0].hover = HoverText::new("Out", "The signal.", "out");
    let mut values = PanelValues::new();
    values.set(
        1,
        1,
        WidgetValue {
            position: 1.0,
            text: "Gate: 1.00".to_string(),
            ..WidgetValue::default()
        },
    );
    let scene = Scene::new(vec![m, meter], Vec::new());
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    // The pointer rests on the middle of a display widget. The hit is
    // the tile, so a press there still moves it.
    let resting_on = |scene: &Scene, module: u32, widget: usize| {
        let point = scene.module(module).unwrap().widget_bounds(widget).unwrap().center();
        let hit = scene.hit(point, 8.0);
        assert_eq!(hit, Hit::Module(module), "a display belongs to the tile");
        CanvasInput {
            hover: Some(hit),
            cursor: Some(point),
            ..CanvasInput::default()
        }
    };
    assert_eq!(
        canvas.hover_target(&resting_on(&scene, 1, 1)),
        Some(HoverTarget::Control(1, 1))
    );
    assert_eq!(
        canvas.hover_target(&resting_on(&scene, 2, 0)),
        Some(HoverTarget::Control(2, 0))
    );
    let (_, hover, value, _) = canvas.hover_content(HoverTarget::Control(1, 1)).unwrap();
    assert_eq!(hover.description, "Lit while a key is held.");
    assert_eq!(value, Some("Gate: 1.00"));
    // A knob's value shows on the knob, not in the hover text.
    let (_, _, value, _) = canvas.hover_content(HoverTarget::Control(1, 0)).unwrap();
    assert_eq!(value, None);

    // A light with no description shows none.
    let scene = Scene::new(vec![plain], Vec::new());
    let canvas = CanvasModel::new(&scene, &none);
    assert_eq!(canvas.hover_target(&resting_on(&scene, 1, 1)), None);
}

#[test]
fn no_hover_text_during_a_drag() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let port = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let drags = [
        Interaction::Wiring {
            from: port,
            pulled: false,
        },
        Interaction::Turning {
            module: 1,
            widget: 0,
            drag: ValueDrag::KNOB,
            last: 0.0,
            position: 0.5,
        },
        Interaction::Moving {
            ids: vec![1],
            start: Point::ORIGIN,
            applied: Vector::ZERO,
            click: None,
        },
        Interaction::Panning {
            last: Point::ORIGIN,
            click: None,
            start: Camera::default(),
        },
    ];
    for interaction in drags {
        let state = CanvasInput {
            hover: Some(Hit::Port(port)),
            interaction: interaction.clone(),
            ..CanvasInput::default()
        };
        assert_eq!(canvas.hover_target(&state), None, "{interaction:?}");
    }
}

#[test]
fn tab_focus_shows_hover_text_but_a_click_does_not() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let focused = |tab_focus| CanvasInput {
        focus: FocusState::at(1, 2, tab_focus, FocusMode::Browse),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.hover_target(&focused(true)), Some(HoverTarget::Control(1, 2)));
    assert_eq!(canvas.hover_target(&focused(false)), None);
    // With the pointer resting on the knob, a Tab press to the toggle
    // shows the toggle's text until the pointer moves.
    let knob = Hit::Widget { module: 1, widget: 0 };
    let tabbed = CanvasInput {
        hover: Some(knob),
        focus_first: true,
        ..focused(true)
    };
    assert_eq!(canvas.hover_target(&tabbed), Some(HoverTarget::Control(1, 2)));
    let moved = CanvasInput {
        hover: Some(knob),
        focus_first: false,
        ..focused(true)
    };
    assert_eq!(canvas.hover_target(&moved), Some(HoverTarget::Control(1, 0)));
    // Focus on a module that is no longer selected shows nothing.
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    assert_eq!(canvas.hover_target(&focused(true)), None);
}

fn cable(from: u32, to: u32) -> CableView {
    CableView {
        from: (from, 0),
        to: (to, 0),
        colour_from: Color::WHITE,
        colour_to: Color::WHITE,
        delayed: false,
    }
}

/// Modules 1 -> 2 -> 3, so selecting 1 focuses one cable and selecting
/// 2 focuses both.
fn chain() -> Scene {
    Scene::new(
        vec![module(1, 0.0), module(2, 200.0), module(3, 400.0)],
        vec![cable(1, 2), cable(2, 3)],
    )
}

/// Activity fills both ends of a cable without changing cable geometry.
#[test]
fn activity_fills_connected_ports_from_overlay() {
    let scene = chain();
    let selection = BTreeSet::new();
    let idle = CanvasModel::new(&scene, &selection);
    let active = CanvasModel::new(&scene, &selection).activity([(1, 0)]);
    let state = CanvasInput::default();
    let before = idle.overlay(&state, &LinesCache::default());
    let after = active.overlay(&state, &LinesCache::default());
    assert_eq!(after.cables.len(), before.cables.len());
    assert_eq!(after.shapes.len(), before.shapes.len() + 2);
    assert!(after.shapes.iter().take(2).all(|shape| shape.fill()[3] > 0.0));
}

#[test]
fn selected_modules_bring_their_cables_forward() {
    let scene = chain();
    let state = CanvasInput::default();

    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    assert!(canvas.overlay(&state, &LinesCache::default()).cables.is_empty());

    let first = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &first);
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 1);

    let middle = BTreeSet::from([2]);
    let canvas = CanvasModel::new(&scene, &middle);
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 2);
}

#[test]
fn tiles_stay_draggable_when_zoomed_out() {
    // At the smallest zoom, the port hit radius must not cover the whole
    // tile, or pressing anywhere on it would start a cable.
    let scene = chain();
    let mut state = CanvasInput::default();
    state.camera.zoom = crate::camera::MIN_ZOOM;
    let centre = scene.module(2).unwrap().bounds().center();
    assert_eq!(scene.hit(centre, state.hit_radius()), Hit::Module(2));
}

#[test]
fn hovering_a_module_brings_its_cables_forward() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let state = CanvasInput {
        hover: Some(Hit::Module(3)),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 1);
}

#[test]
fn hovering_a_port_brings_only_its_cables_forward() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    // Module 2's input carries cable 1 -> 2; its output carries 2 -> 3.
    let state = CanvasInput {
        hover: Some(Hit::Port(PortRef {
            module: 2,
            output: false,
            index: 0,
        })),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 1);
    // A port with no cable highlights nothing.
    let state = CanvasInput {
        hover: Some(Hit::Port(PortRef {
            module: 1,
            output: false,
            index: 0,
        })),
        ..CanvasInput::default()
    };
    assert!(canvas.overlay(&state, &LinesCache::default()).cables.is_empty());
}

#[test]
fn hovering_a_cable_brings_only_that_cable_forward() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let state = CanvasInput {
        hover: Some(Hit::Cable(1)),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 1);
}

fn button_module() -> ModuleView {
    let mut module = module(1, 0.0);
    module.panel = PanelView {
        width: 6,
        height: 3,
        widgets: vec![widget(WidgetViewKind::Button, 0)],
    };
    module
}

fn drive(canvas: &CanvasModel<'_>, state: &mut CanvasInput, event: &Event, at: Point) -> Vec<CanvasEvent> {
    drive_at(canvas, state, Some(event), at, Instant::now())
}

/// Sends one event, or `None` for something that means nothing to the
/// canvas, with the pointer at `at` and the clock at `now`. Returns what
/// the canvas reported.
fn drive_at(
    canvas: &CanvasModel<'_>,
    state: &mut CanvasInput,
    event: Option<&Event>,
    at: Point,
    now: Instant,
) -> Vec<CanvasEvent> {
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut out = Reaction::default();
    canvas.handle(state, event, Some(at), bounds, now, &mut out);
    out.events
}

fn click(button: PointerButton) -> Event {
    Event::Pointer(PointerEvent::Pressed(button))
}

/// A select click opens a list and a list choice sets one value. Dragging
/// the closed selector still moves its tile without changing sound.
#[test]
fn select_popup_preserves_choice_and_tile_gestures() {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Select {
                options: vec!["ADSR".into(), "AD".into()],
            },
            0,
        )],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        centre,
    );
    assert!(state.focus.popup().is_some());
    let list = canvas
        .select_popup_bounds(&state, Rect::new(Point::ORIGIN, Size::new(800.0, 600.0)))
        .unwrap();
    let choice = Point::new(list.x + 20.0, list.y + SELECT_ROW_PX * 1.5);
    let picked = drive(&canvas, &mut state, &click(PointerButton::Left), choice);
    assert!(picked.contains(&CanvasEvent::SetControl {
        module: 1,
        widget: 0,
        position: 1.0,
        part: ControlPart::Main
    }));
    assert!(state.focus.popup().is_none());

    // Scrolling dismisses the open list without changing the canvas zoom.
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        centre,
    );
    assert!(state.focus.popup().is_some());
    let zoom = state.camera.zoom;
    let events = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 })),
        centre,
    );
    assert!(events.is_empty());
    assert!(state.focus.popup().is_none());
    assert_eq!(state.camera.zoom, zoom);

    drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    let moved = Point::new(centre.x + GRID * 2.0, centre.y);
    let events = drive(&canvas, &mut state, &Event::Pointer(PointerEvent::Moved), moved);
    assert!(events.iter().any(|event| matches!(event, CanvasEvent::Move { .. })));
    let released = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        moved,
    );
    assert!(
        !released
            .iter()
            .any(|event| matches!(event, CanvasEvent::SetControl { .. }))
    );
    assert!(state.focus.popup().is_none());
}

/// Focused select lists open with Space, move the highlight with vertical
/// arrows, and commit with Enter. Other navigation cannot edit or move
/// focus behind the list. Escape dismisses it without an edit.
#[test]
fn select_popup_keyboard_choices_are_explicit() {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Select {
                options: vec!["ADSR".into(), "AD".into()],
            },
            0,
        )],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let at = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    assert!(drive(&canvas, &mut state, &key_event(Key::Space, true), at).is_empty());
    assert!(state.focus.popup().is_some());
    assert!(drive(&canvas, &mut state, &key_event(Key::Down, true), at).is_empty());
    for key in [Key::Left, Key::Right, Key::Tab] {
        assert!(drive(&canvas, &mut state, &key_event(key, true), at).is_empty());
        assert_eq!(state.focus.control(), Some((1, 0)));
        assert_eq!(state.focus.popup().map(|popup| popup.highlight), Some(1));
    }
    let chosen = drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    assert_eq!(
        chosen,
        vec![CanvasEvent::SetControl {
            module: 1,
            widget: 0,
            position: 1.0,
            part: ControlPart::Main
        }]
    );
    assert!(state.focus.popup().is_none());
    drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert!(drive(&canvas, &mut state, &key_event(Key::Escape, true), at).is_empty());
    assert!(state.focus.popup().is_none());
}

/// When a mode change hides the focused select with its list open, the
/// list closes, and the next arrow or Enter edits nothing and moves no
/// tile (ADR-0030).
#[test]
fn hiding_the_focused_select_closes_it_and_uses_up_the_key() {
    let mut view = module(1, 0.0);
    let mut select = widget(
        WidgetViewKind::Select {
            options: vec!["ADSR".into(), "AD".into()],
        },
        0,
    );
    select.hidden = true;
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![select],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let at = scene.module(1).unwrap().bounds().center();
    for key in [Key::Down, Key::Enter] {
        let mut state = CanvasInput {
            focus: FocusState::at(
                1,
                0,
                false,
                FocusMode::Popup(SelectPopup {
                    highlight: 1,
                    scroll: 0,
                    wheel: 0.0,
                }),
            ),
            ..CanvasInput::default()
        };
        let events = drive(&canvas, &mut state, &key_event(key, true), at);
        assert!(events.is_empty(), "{key:?}: {events:?}");
        assert!(state.focus.popup().is_none() && state.focus.control().is_none());
    }
}

/// A removed select cannot swallow the next key meant for the canvas.
#[test]
fn stale_select_popup_passes_escape_to_normal_key_handling() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let at = scene.module(1).unwrap().bounds().center();
    for module in [1, 999] {
        let mut state = CanvasInput {
            focus: FocusState::at(
                module,
                999,
                false,
                FocusMode::Popup(SelectPopup {
                    highlight: 0,
                    scroll: 0,
                    wheel: 0.0,
                }),
            ),
            ..CanvasInput::default()
        };
        let events = drive(&canvas, &mut state, &key_event(Key::Escape, true), at);
        assert_eq!(events, vec![CanvasEvent::Select(Vec::new())]);
        assert!(state.focus.popup().is_none());
    }
}

/// A press on empty canvas with a stale list goes on to normal handling, so
/// the removed widget's list cannot swallow it. The same holds for the wheel,
/// which then zooms.
#[test]
fn stale_select_popup_passes_the_pointer_to_normal_handling() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let stale = || {
        FocusState::at(
            999,
            0,
            false,
            FocusMode::Popup(SelectPopup {
                highlight: 0,
                scroll: 0,
                wheel: 0.0,
            }),
        )
    };
    let empty = Point::new(790.0, 590.0);
    let mut state = CanvasInput {
        focus: stale(),
        ..CanvasInput::default()
    };
    drive(&canvas, &mut state, &click(PointerButton::Left), empty);
    assert!(state.focus.popup().is_none());
    assert!(matches!(state.interaction, Interaction::Selecting { .. }));

    let mut state = CanvasInput {
        focus: stale(),
        ..CanvasInput::default()
    };
    let zoom = state.camera.zoom;
    drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 })),
        empty,
    );
    assert_ne!(state.camera.zoom, zoom, "the wheel zooms again");
}

/// An open list commits to its own widget, even when keyboard focus has moved
/// to another control. A press outside the canvas, or the window losing focus,
/// closes it with no edit.
#[test]
fn select_popup_commits_to_its_own_widget_and_closes_with_no_edit() {
    let select = || WidgetViewKind::Select {
        options: vec!["ADSR".into(), "AD".into()],
    };
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 12,
        height: 2,
        widgets: vec![widget(select(), 0), widget(select(), 6)],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let at = scene.module(1).unwrap().widget_bounds(1).unwrap().center();
    // The list is the focused control's own, whatever had the focus before.
    let open = || {
        FocusState::at(
            1,
            0,
            false,
            FocusMode::Popup(SelectPopup {
                highlight: 1,
                scroll: 0,
                wheel: 0.0,
            }),
        )
    };
    let mut state = CanvasInput {
        focus: open(),
        ..CanvasInput::default()
    };
    let chosen = drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    assert_eq!(
        chosen,
        vec![CanvasEvent::SetControl {
            module: 1,
            widget: 0,
            position: 1.0,
            part: ControlPart::Main
        }]
    );

    let mut state = CanvasInput {
        focus: open(),
        ..CanvasInput::default()
    };
    let unfocused = drive(&canvas, &mut state, &Event::WindowUnfocused, at);
    assert!(state.focus.popup().is_none());
    assert!(!unfocused.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. })));
}

/// A canvas with one open select list of `count` choices named `c0`, `c1`,
/// and so on.
fn long_list(count: usize) -> Scene {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Select {
                options: (0..count).map(|i| format!("c{i}")).collect(),
            },
            0,
        )],
    };
    Scene::new(vec![view], Vec::new())
}

/// A key press that carries typed text, as a real keyboard sends it.
fn typed(key: Key, text: &str) -> Event {
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers: Modifiers::default(),
        text: Some(text.into()),
        repeat: false,
    })
}

/// Over 12 choices the list shows 12 rows and scrolls: the wheel moves it,
/// a press picks the row under the pointer, and Down keeps the highlight in
/// view.
#[test]
fn a_long_select_list_scrolls() {
    let scene = long_list(30);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let canvas_area = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let box_at = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), box_at);
    drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        box_at,
    );
    let list = canvas.select_popup_bounds(&state, canvas_area).unwrap();
    assert_eq!(list.height, SELECT_ROW_PX * 12.0, "12 rows show");

    let inside = Point::new(list.x + 20.0, list.y + SELECT_ROW_PX * 0.5);
    let wheel = |y| Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y }));
    let zoom = state.camera.zoom;
    drive(&canvas, &mut state, &wheel(-3.0), inside);
    assert_eq!(state.focus.popup().copied().unwrap().scroll, 3);
    assert_eq!(state.camera.zoom, zoom, "the wheel does not zoom over a list");

    // Small trackpad steps add up to a whole row.
    let pixels = Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Pixels {
        x: 0.0,
        y: -SELECT_ROW_PX * 0.6,
    }));
    drive(&canvas, &mut state, &pixels, inside);
    assert_eq!(state.focus.popup().copied().unwrap().scroll, 3);
    drive(&canvas, &mut state, &pixels, inside);
    assert_eq!(state.focus.popup().copied().unwrap().scroll, 4);

    // It stops at either end.
    drive(&canvas, &mut state, &wheel(-100.0), inside);
    assert_eq!(state.focus.popup().copied().unwrap().scroll, 18);
    drive(&canvas, &mut state, &wheel(100.0), inside);
    assert_eq!(state.focus.popup().copied().unwrap().scroll, 0);

    // A press picks the row it is on, counting what has scrolled away.
    drive(&canvas, &mut state, &wheel(-5.0), inside);
    let picked = drive(&canvas, &mut state, &click(PointerButton::Left), inside);
    assert!(picked.contains(&CanvasEvent::SetControl {
        module: 1,
        widget: 0,
        position: 5.0,
        part: ControlPart::Main
    }));

    // The keyboard scrolls the highlight into view.
    let mut state = CanvasInput::default();
    drive_at(&canvas, &mut state, None, box_at, Instant::now());
    canvas.open_select(&mut state, 1, 0);
    for _ in 0..15 {
        drive(&canvas, &mut state, &key_event(Key::Down, true), box_at);
    }
    let popup = state.focus.popup().copied().unwrap();
    assert_eq!((popup.highlight, popup.scroll), (15, 4));
}

/// The list stays inside a short canvas, and a list that is too short to
/// scroll takes the wheel and stays open.
#[test]
fn a_select_list_fits_the_canvas_and_a_short_one_keeps_the_wheel() {
    assert_eq!(select_popup::select_rows(100.0, 30), 4);
    assert_eq!(select_popup::select_rows(5.0, 30), 1, "always one row");
    assert_eq!(select_popup::select_rows(600.0, 3), 3);

    let scene = long_list(3);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        size: Size::new(800.0, 600.0),
        ..CanvasInput::default()
    };
    canvas.open_select(&mut state, 1, 0);
    let list = canvas
        .select_popup_bounds(&state, Rect::new(Point::ORIGIN, Size::new(800.0, 600.0)))
        .unwrap();
    let inside = Point::new(list.x + 20.0, list.y + 5.0);
    drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: -1.0 })),
        inside,
    );
    let popup = state.focus.popup().copied().expect("stays open");
    assert_eq!(popup.scroll, 0);
}

/// When the canvas grows with a scrolled list open, the scroll comes back, so
/// the list has no blank rows and a press on its last row picks the last value.
#[test]
fn a_list_on_a_grown_canvas_has_no_blank_rows() {
    let scene = long_list(30);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let area = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    // Scrolled to the end on a canvas 4 rows tall.
    let mut state = CanvasInput {
        size: Size::new(800.0, 4.0 * SELECT_ROW_PX),
        ..CanvasInput::default()
    };
    canvas.open_select(&mut state, 1, 0);
    state.focus.popup_mut().unwrap().scroll = 26;

    // Then `drive` gives it a canvas with room for all 12 rows.
    state.size = area.size();
    let list = canvas.select_popup_bounds(&state, area).unwrap();
    let last = Point::new(list.x + 20.0, list.y + list.height - 1.0);
    let events = drive(&canvas, &mut state, &click(PointerButton::Left), last);
    assert!(
        events.contains(&CanvasEvent::SetControl {
            module: 1,
            widget: 0,
            position: 29.0,
            part: ControlPart::Main
        }),
        "{events:?}"
    );
}

/// Typing jumps to the next choice that starts with the text, ignoring case
/// and wrapping round. A key that matches nothing is used up, so `a` does not
/// open the browser behind the list, and a key with Command still passes.
#[test]
fn typing_in_an_open_list_jumps_to_the_next_match() {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Select {
                options: vec!["Sine".into(), "Saw".into(), "Tri".into(), "1/16".into()],
            },
            0,
        )],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    canvas.open_select(&mut state, 1, 0);
    let letter = |c: &str| typed(key_of(c), c);
    let highlight = |state: &CanvasInput| state.focus.popup().copied().unwrap().highlight;

    drive(&canvas, &mut state, &letter("s"), at);
    assert_eq!(highlight(&state), 1, "searches from after the highlight");
    drive(&canvas, &mut state, &letter("S"), at);
    assert_eq!(highlight(&state), 0, "ignores case and wraps round");
    drive(&canvas, &mut state, &letter("1"), at);
    assert_eq!(highlight(&state), 3, "any character, so 1/16 can be found");

    let events = drive(&canvas, &mut state, &letter("a"), at);
    assert!(events.is_empty(), "no match edits nothing: {events:?}");
    assert_eq!(highlight(&state), 3);
    assert!(state.focus.popup().is_some());

    // Backspace sends a control character, and must not delete the module.
    let events = drive(&canvas, &mut state, &typed(Key::Backspace, "\u{8}"), at);
    assert!(events.is_empty(), "{events:?}");
}

/// A fader tile with one modulated fader: `vertical` or not.
fn fader_scene(vertical: bool) -> (Scene, PanelValues) {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 6,
        widgets: vec![widget(WidgetViewKind::Fader { vertical, steps: 0 }, 0)],
    };
    let mut values = PanelValues::new();
    values.set(
        1,
        0,
        WidgetValue {
            position: 0.25,
            modulation: crate::ModulationMark::Range { low: 0.1, high: 0.4 },
            ..WidgetValue::default()
        },
    );
    (Scene::new(vec![view], Vec::new()), values)
}

/// During a drag the cursor shows the drag's own direction, as it did on hover.
#[test]
fn a_fader_drag_keeps_its_cursor() {
    for (vertical, expected) in [
        (true, CursorShape::ResizeVertical),
        (false, CursorShape::ResizeHorizontal),
    ] {
        let (scene, values) = fader_scene(vertical);
        let selected = BTreeSet::from([1]);
        let canvas = CanvasModel::new(&scene, &selected).values(&values);
        let rect = scene.module(1).unwrap().widget_bounds(0).unwrap();
        let at = crate::controls::fader_track(rect, vertical).center();
        let mut state = CanvasInput::default();
        drive(&canvas, &mut state, &click(PointerButton::Left), at);
        assert!(matches!(state.interaction, Interaction::Turning { .. }));
        assert_eq!(state.interaction.cursor(), Some(expected));
    }
    assert_eq!(Interaction::Idle.cursor(), None);
}

/// Both faders change their value along the track, relative: pressing moves
/// nothing, and a drag across the track moves nothing either. A press on the
/// bar beside the track drags depth, and the value stays.
#[test]
fn a_fader_drags_along_its_track_and_its_bar_drags_depth() {
    for vertical in [true, false] {
        let (scene, values) = fader_scene(vertical);
        let selected = BTreeSet::from([1]);
        let canvas = CanvasModel::new(&scene, &selected).values(&values);
        let rect = scene.module(1).unwrap().widget_bounds(0).unwrap();
        let at = crate::controls::fader_track(rect, vertical).center();
        let way = |d: f32| {
            if vertical {
                Point::new(at.x, at.y - d)
            } else {
                Point::new(at.x + d, at.y)
            }
        };
        // The place travels beside the event, so a move itself says nothing of it.
        let moved = |_to: Point| Event::Pointer(PointerEvent::Moved);
        let released = Event::Pointer(PointerEvent::Released(PointerButton::Left));

        let mut state = CanvasInput::default();
        let pressed = drive(&canvas, &mut state, &click(PointerButton::Left), at);
        assert!(
            !pressed.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. })),
            "a press moves nothing"
        );
        let events = drive(&canvas, &mut state, &moved(way(100.0)), way(100.0));
        assert!(
            events.contains(&CanvasEvent::SetControl {
                module: 1,
                widget: 0,
                position: 0.75,
                part: ControlPart::Main
            }),
            "100 of 200 pixels adds half the range: {events:?}"
        );
        drive(&canvas, &mut state, &released, way(100.0));

        // Across the track, the value stays.
        let across = if vertical {
            Point::new(at.x + 40.0, at.y)
        } else {
            Point::new(at.x, at.y + 40.0)
        };
        let mut state = CanvasInput::default();
        drive(&canvas, &mut state, &click(PointerButton::Left), at);
        let events = drive(&canvas, &mut state, &moved(across), across);
        assert!(!events.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. })));
        drive(&canvas, &mut state, &released, across);

        // The bar drags depth.
        let bar = crate::controls::fader_bar_point(rect, vertical, 0.5);
        let on_bar = |d: f32| {
            if vertical {
                Point::new(bar.x, bar.y - d)
            } else {
                Point::new(bar.x + d, bar.y)
            }
        };
        let mut state = CanvasInput::default();
        drive(&canvas, &mut state, &click(PointerButton::Left), bar);
        assert!(matches!(state.interaction, Interaction::Depth { .. }));
        let events = drive(&canvas, &mut state, &moved(on_bar(20.0)), on_bar(20.0));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CanvasEvent::DragDepth { delta, .. } if (delta - 0.1).abs() < 1e-5)),
            "{events:?}"
        );
        assert!(!events.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. })));
    }
}

/// A fader takes the keys a knob takes: arrows change its value, in 1% steps
/// or one whole number, and Tab focuses it.
#[test]
fn a_fader_takes_the_keys_of_a_knob() {
    let (scene, values) = fader_scene(true);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let events = drive(&canvas, &mut state, &key_event(Key::Up, true), at);
    assert!(events.contains(&CanvasEvent::SetControl {
        module: 1,
        widget: 0,
        position: 0.26,
        part: ControlPart::Main
    }));
    assert_eq!(focusable(&scene.module(1).unwrap().panel), [0]);
    assert_eq!(
        scene.module(1).unwrap().panel.widgets[0].size(),
        (2, 6),
        "a vertical fader is tall"
    );
}

/// A selected tile with one knob, focused, with its value text set.
fn typed_scene() -> (Scene, PanelValues) {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 3,
        height: 3,
        widgets: vec![widget(WidgetViewKind::Knob { steps: 0 }, 0)],
    };
    let mut values = PanelValues::new();
    values.set(
        1,
        0,
        WidgetValue {
            position: 0.5,
            text: "128.0 BPM".into(),
            ..WidgetValue::default()
        },
    );
    (Scene::new(vec![view], Vec::new()), values)
}

/// A key press with `key` and `text` set apart, and modifiers held. On a layout
/// such as AZERTY a digit's key is not the digit.
fn typed_key(key: Key, text: Option<&str>, modifiers: Modifiers, repeat: bool) -> Event {
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers,
        text: text.map(Into::into),
        repeat,
    })
}

/// Types text into the canvas, one character per key press.
fn type_text(canvas: &CanvasModel<'_>, state: &mut CanvasInput, text: &str, at: Point) -> Vec<CanvasEvent> {
    let mut events = Vec::new();
    for c in text.chars() {
        let c = c.to_string();
        events.extend(drive(canvas, state, &typed(key_of(&c), &c), at));
    }
    events
}

/// While an app text field has focus the canvas takes no key press, so a key
/// that edits the text never moves, deletes, or runs anything. Tab still moves
/// focus, and the pointer still works.
#[test]
fn a_focused_text_field_keeps_keys_from_the_canvas() {
    use Key;
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected)
        .values(&values)
        .focus_policy(CanvasFocusPolicy {
            text_field_keys: true,
            ..CanvasFocusPolicy::default()
        });
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput::default();
    for key in [Key::Enter, Key::Left, Key::Space, Key::Escape, Key::Backspace] {
        let events = drive(&canvas, &mut state, &key_event(key, true), at);
        assert!(events.is_empty(), "{key:?} did nothing, but sent {events:?}");
    }
    assert_eq!(state.focus.control(), None, "Enter did not focus a control");

    let paste = typed_key(Key::Character('v'), None, command(), false);
    assert!(drive(&canvas, &mut state, &paste, at).is_empty(), "the field pastes");

    let events = drive(&canvas, &mut state, &key_event(Key::Tab, true), at);
    assert!(!events.is_empty(), "Tab still moves focus");
    assert!(!drive(&canvas, &mut state, &click(PointerButton::Left), at).is_empty());
}

/// A press outside the canvas with a list open also takes the keys from the
/// canvas, as it does with no list open.
#[test]
fn a_press_outside_closes_a_select_list_and_clears_focus() {
    let scene = long_list(3);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        size: Size::new(800.0, 600.0),
        ..CanvasInput::default()
    };
    canvas.open_select(&mut state, 1, 0);
    let events = drive(
        &canvas,
        &mut state,
        &click(PointerButton::Left),
        Point::new(-10.0, -10.0),
    );
    assert!(events.is_empty());
    assert!(state.focus.popup().is_none());
    assert_eq!(state.focus.control(), None);
}

/// A select list stays inside the canvas with its box panned off either edge.
#[test]
fn a_select_list_stays_in_the_canvas_when_its_box_is_off_an_edge() {
    let scene = long_list(5);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let area = Rect::new(Point::ORIGIN, Size::new(400.0, 300.0));
    for pan in [-5000.0, 5000.0] {
        let mut state = CanvasInput {
            size: area.size(),
            ..CanvasInput::default()
        };
        state.camera.pan(Vector::new(0.0, pan));
        canvas.open_select(&mut state, 1, 0);
        let list = canvas.select_popup_bounds(&state, area).unwrap();
        assert!(list.y >= 0.0 && list.y + list.height <= area.height, "{pan}: {list:?}");
    }
}

/// During a drag no field opens: the drag's next move would overwrite the
/// typed value, and the commit would join the drag's undo step.
#[test]
fn no_typed_field_opens_during_a_drag() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        interaction: Interaction::Panning {
            last: Point::ORIGIN,
            click: None,
            start: Camera::default(),
        },
        ..CanvasInput::default()
    };
    type_text(&canvas, &mut state, "5", at);
    assert!(state.focus.typed().is_none());
}

/// The reason for bad text goes below the field, above it at the canvas's
/// bottom edge, and never past the right edge.
#[test]
fn the_reason_for_bad_text_stays_in_the_canvas() {
    let canvas = Rect::new(Point::new(10.0, 20.0), Size::new(400.0, 300.0));
    let field = |x, y| Rect::new(Point::new(x, y), Size::new(110.0, 23.0));

    let middle = typed_field::typed_reason_bounds(field(50.0, 100.0), 120.0, canvas);
    assert!(middle.y > 100.0, "below");
    assert_eq!(middle.x, 50.0);

    let bottom = typed_field::typed_reason_bounds(field(50.0, 297.0), 120.0, canvas);
    assert!(bottom.y < 297.0, "above");

    let right = typed_field::typed_reason_bounds(field(300.0, 100.0), 200.0, canvas);
    assert!(right.x + right.width <= canvas.x + canvas.width);

    let wide = typed_field::typed_reason_bounds(field(300.0, 100.0), 900.0, canvas);
    assert_eq!((wide.x, wide.width), (canvas.x, canvas.width), "a long reason is cut");
}

/// A digit opens the field holding it, and Enter commits what was typed. Left,
/// Right, Backspace, and Delete edit at the caret.
#[test]
fn a_digit_opens_the_typed_field_and_enter_commits() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    assert!(type_text(&canvas, &mut state, "129", at).is_empty());
    assert_eq!(state.focus.typed().unwrap().draft.text, "129");

    let named = |key| key_event(key, true);
    use Key;
    drive(&canvas, &mut state, &named(Key::Left), at);
    drive(&canvas, &mut state, &named(Key::Backspace), at);
    assert_eq!(state.focus.typed().unwrap().draft.text, "19", "Backspace takes the 2");
    drive(&canvas, &mut state, &named(Key::Home), at);
    drive(&canvas, &mut state, &named(Key::Delete), at);
    assert_eq!(state.focus.typed().unwrap().draft.text, "9", "Delete takes the 1");
    drive(&canvas, &mut state, &typed(Key::Character('1'), "1"), at);
    drive(&canvas, &mut state, &named(Key::End), at);
    drive(&canvas, &mut state, &typed(Key::Character('0'), "0"), at);
    assert_eq!(state.focus.typed().unwrap().draft.text, "190");

    let committed = drive(&canvas, &mut state, &named(Key::Enter), at);
    assert_eq!(
        committed,
        vec![CanvasEvent::TypedValue {
            module: 1,
            widget: 0,
            text: "190".into()
        }]
    );
    assert!(state.focus.typed().is_none());
    assert_eq!(state.focus.control(), Some((1, 0)), "focus stays on the control");
}

/// The first key reads the typed text, not the key, so a layout where a digit
/// needs Shift still opens the field. A minus sign, a decimal point, and a comma
/// each start a number. Letters do not.
#[test]
fn the_field_opens_on_what_was_typed_not_on_the_key() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let none = Modifiers::default();
    for (key, text, opens) in [
        ("&", "1", true),
        ("-", "-", true),
        (".", ".", true),
        (",", ",", true),
        ("1", "&", false),
        ("x", "x", false),
    ] {
        let mut state = CanvasInput {
            focus: FocusState::at(1, 0, false, FocusMode::Browse),
            ..CanvasInput::default()
        };
        let event = typed_key(key_of(key), Some(text), none, false);
        drive(&canvas, &mut state, &event, at);
        assert_eq!(state.focus.typed().is_some(), opens, "{key} {text}");
        if opens {
            assert_eq!(state.focus.typed().unwrap().draft.text, text);
        }
    }
    // With Command, a digit or a dot stays a shortcut, so Cmd+. can panic.
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let event = typed_key(Key::Character('.'), Some("."), command(), false);
    drive(&canvas, &mut state, &event, at);
    assert!(state.focus.typed().is_none());
    // A control that does not take numbers ignores a digit.
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &typed(Key::Character('1'), "1"), at);
    assert!(state.focus.typed().is_none(), "no focus, no field");
}

/// While the field is open it takes every key: Backspace does not delete the
/// module, and `f` does not frame the view. A held Enter does nothing, so it
/// cannot commit and open again. Tab closes it, and then moves on as usual.
#[test]
fn the_open_field_takes_every_key() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    use Key;
    type_text(&canvas, &mut state, "1", at);
    let camera = state.camera;
    for event in [
        key_event(Key::Backspace, true),
        key_event(Key::Delete, true),
        typed(Key::Character('f'), "f"),
        typed(Key::Character('a'), "a"),
        typed_key(Key::Character('v'), Some("v"), command(), false),
        key_event(Key::Up, true),
    ] {
        let events = drive(&canvas, &mut state, &event, at);
        assert!(events.is_empty(), "{event:?}: {events:?}");
    }
    assert_eq!(state.camera, camera);
    assert_eq!(
        state.focus.typed().unwrap().draft.text,
        "fa",
        "letters are text; Cmd+V is not"
    );

    let held = typed_key(Key::Enter, None, Modifiers::default(), true);
    assert!(drive(&canvas, &mut state, &held, at).is_empty());
    assert!(state.focus.typed().is_some(), "a repeat does not commit");

    let tabbed = drive(&canvas, &mut state, &key_event(Key::Tab, true), at);
    assert!(state.focus.typed().is_none());
    assert_eq!(tabbed, vec![CanvasEvent::Select(vec![1])], "Tab moves on as usual");
}

/// Enter opens the field with the value text selected, and Enter again closes
/// it with no event: the text is rounded, so committing it would change the
/// value. A character replaces the selection. Escape cancels.
#[test]
fn enter_on_unchanged_text_commits_nothing() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    use Key;
    assert!(drive(&canvas, &mut state, &key_event(Key::Enter, true), at).is_empty());
    assert_eq!(state.focus.typed().unwrap().draft.text, "128.0 BPM");
    assert!(drive(&canvas, &mut state, &key_event(Key::Enter, true), at).is_empty());
    assert!(state.focus.typed().is_none(), "closed, with no edit");

    drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    type_text(&canvas, &mut state, "9", at);
    assert_eq!(
        state.focus.typed().unwrap().draft.text,
        "9",
        "the selection is replaced"
    );
    assert!(drive(&canvas, &mut state, &key_event(Key::Escape, true), at).is_empty());
    assert!(state.focus.typed().is_none());
    assert_eq!(state.focus.control(), Some((1, 0)), "Escape closes only the field");

    // Selected text and Backspace leave an empty field, and the next character
    // goes in it.
    drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    drive(&canvas, &mut state, &key_event(Key::Backspace, true), at);
    assert_eq!(state.focus.typed().unwrap().draft.text, "");
}

/// Bad text stays open and Enter does nothing. It can be corrected, and then
/// Enter commits.
#[test]
fn bad_typed_text_blocks_enter_until_it_is_corrected() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected)
        .values(&values)
        .typed_check(|_, _, text| {
            text.parse::<f32>()
                .map(|_| ())
                .map_err(|_| "Not a number in Hz".to_string())
        });
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    use Key;
    type_text(&canvas, &mut state, "1a", at);
    assert_eq!(state.focus.typed().unwrap().draft.error(), Some("Not a number in Hz"));
    assert!(drive(&canvas, &mut state, &key_event(Key::Enter, true), at).is_empty());
    assert!(state.focus.typed().is_some(), "bad text keeps the field open");
    drive(&canvas, &mut state, &key_event(Key::Backspace, true), at);
    assert!(state.focus.typed().unwrap().draft.error().is_none());
    assert_eq!(drive(&canvas, &mut state, &key_event(Key::Enter, true), at).len(), 1);
}

/// A press outside, the wheel, the window losing focus, or the widget going
/// each close the field with no value, and the event still does its work. A
/// press on the field is used up, and stays open.
#[test]
fn moving_focus_away_closes_the_field_with_no_change() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let at = scene.module(1).unwrap().bounds().center();
    let opened = || {
        let mut state = CanvasInput {
            focus: FocusState::at(1, 0, false, FocusMode::Browse),
            ..CanvasInput::default()
        };
        type_text(&canvas, &mut state, "5", at);
        state
    };
    let edits = |events: &[CanvasEvent]| {
        events
            .iter()
            .any(|e| matches!(e, CanvasEvent::TypedValue { .. } | CanvasEvent::SetControl { .. }))
    };

    let mut state = opened();
    let area = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let on_field = canvas.typed_field_bounds(&state, area).unwrap().center();
    assert!(!edits(&drive(
        &canvas,
        &mut state,
        &click(PointerButton::Left),
        on_field
    )));
    assert!(state.focus.typed().is_some(), "a press on the field stays");
    assert!(matches!(state.interaction, Interaction::Idle));

    let mut state = opened();
    let empty = Point::new(790.0, 590.0);
    assert!(!edits(&drive(&canvas, &mut state, &click(PointerButton::Left), empty)));
    assert!(state.focus.typed().is_none());
    assert!(
        matches!(state.interaction, Interaction::Selecting { .. }),
        "the press still works"
    );

    let mut state = opened();
    let zoom = state.camera.zoom;
    let wheel = Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 }));
    assert!(!edits(&drive(&canvas, &mut state, &wheel, at)));
    assert!(state.focus.typed().is_none());
    assert_ne!(state.camera.zoom, zoom, "the wheel still zooms");

    let mut state = opened();
    assert!(!edits(&drive(&canvas, &mut state, &Event::WindowUnfocused, at)));
    assert!(state.focus.typed().is_none());

    // The module has gone from the selection.
    let mut state = opened();
    let none = BTreeSet::new();
    let unselected = CanvasModel::new(&scene, &none).values(&values);
    assert!(!edits(&drive(
        &unselected,
        &mut state,
        &key_event(Key::Enter, true),
        at
    )));
    assert!(state.focus.typed().is_none());
}

/// A press elsewhere in the window takes the keys from the canvas, so a digit
/// typed in an inspector field cannot open a field on the tile as well.
#[test]
fn a_press_outside_the_canvas_takes_its_focus() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    drive(
        &canvas,
        &mut state,
        &click(PointerButton::Left),
        Point::new(5_000.0, 10.0),
    );
    assert_eq!(state.focus.control(), None);
    type_text(&canvas, &mut state, "5", Point::new(5_000.0, 10.0));
    assert!(state.focus.typed().is_none());
}

/// In MIDI learn mode no field opens: a digit does nothing, and Enter arms.
#[test]
fn no_field_opens_in_midi_learn_mode() {
    let (scene, values) = typed_scene();
    let selected = BTreeSet::from([1]);
    let learn = LearnView::default();
    let canvas = CanvasModel::new(&scene, &selected).values(&values).learn(Some(&learn));
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    assert!(type_text(&canvas, &mut state, "5", at).is_empty());
    assert!(state.focus.typed().is_none());
    let armed = drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    assert_eq!(armed, vec![CanvasEvent::Learn { module: 1, widget: 0 }]);
    assert!(state.focus.typed().is_none());
}

/// A drag in progress when a pop-up opens ends at the next redraw,
/// which reaches the canvas even when the pop-up takes every pointer
/// and key event. A button held by the pointer is released too.
#[test]
fn a_redraw_ends_gestures_and_holds_when_the_canvas_is_not_interactive() {
    let redraw = Event::Redraw;

    let scene = chain();
    let selected = BTreeSet::from([1]);
    let tile = scene.module(1).unwrap().bounds().center();
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    let pressed = drive(&canvas, &mut state, &click(PointerButton::Left), tile);
    assert_eq!(pressed, vec![CanvasEvent::GestureStart]);
    let blocked = CanvasModel::new(&scene, &selected).focus_policy(CanvasFocusPolicy {
        interactive: false,
        ..CanvasFocusPolicy::default()
    });
    let events = drive(&blocked, &mut state, &redraw, tile);
    assert!(events.contains(&CanvasEvent::GestureEnd), "{events:?}");
    assert!(matches!(state.interaction, Interaction::Idle));
    // Nothing is held now, so later redraws send nothing more.
    let again = drive(&blocked, &mut state, &redraw, tile);
    assert!(!again.contains(&CanvasEvent::GestureEnd), "{again:?}");

    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    let blocked = CanvasModel::new(&scene, &selected).focus_policy(CanvasFocusPolicy {
        interactive: false,
        ..CanvasFocusPolicy::default()
    });
    let events = drive(&blocked, &mut state, &redraw, centre);
    assert!(
        events.contains(&CanvasEvent::ButtonReleased { module: 1, widget: 0 }),
        "{events:?}"
    );
}

fn key_a(modifiers: Modifiers) -> Event {
    let key = Key::Character('a');
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers,
        text: None,
        repeat: false,
    })
}

/// `A` alone opens the browser at the pointer. With any modifier it does
/// not (module-browser.md, Rules).
#[test]
fn only_a_with_no_modifier_opens_the_browser() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let at = Point::new(100.0, 300.0);
    let mut state = CanvasInput::default();
    let events = drive(&canvas, &mut state, &key_a(Modifiers::default()), at);
    assert!(
        matches!(events.as_slice(), [CanvasEvent::OpenBrowser { at: p, .. }] if *p == at),
        "{events:?}"
    );
    for modifiers in [SHIFT, CTRL, ALT, LOGO] {
        let events = drive(&canvas, &mut state, &key_a(modifiers), at);
        assert!(
            !events.iter().any(|e| matches!(e, CanvasEvent::OpenBrowser { .. })),
            "{modifiers:?}: {events:?}"
        );
    }
}

/// #16: Cmd/Ctrl+0 over the canvas sets the zoom to 100% from any
/// zoom, keeping the point under the pointer in place. Without the
/// command modifier, 0 does nothing.
#[test]
fn command_zero_resets_the_zoom_to_100_percent() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let zero = |modifiers| {
        let key = Key::Character('0');
        Event::Key(KeyEvent::Pressed {
            key,
            modifiers,
            text: None,
            repeat: false,
        })
    };
    let at = Point::new(300.0, 200.0);
    for start in [crate::camera::MIN_ZOOM, 0.5, 1.0, 2.25, crate::camera::MAX_ZOOM] {
        let mut state = CanvasInput::default();
        state.camera.zoom = start;
        state.camera.origin = Point::new(-40.0, 60.0);
        let before = state.camera.to_world(at);
        drive(&canvas, &mut state, &zero(Modifiers::default()), at);
        assert_eq!(state.camera.zoom, start, "0 alone leaves the zoom at {start}");
        drive(&canvas, &mut state, &zero(command()), at);
        assert_eq!(state.camera.zoom, 1.0, "from {start}");
        let after = state.camera.to_world(at);
        assert!(
            (after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3,
            "from {start}: {before:?} moved to {after:?}"
        );
    }
}

/// Ctrl+V or Cmd+V pastes at the pointer over the canvas, and at the
/// centre of the view with the pointer elsewhere (clipboard.md).
#[test]
fn paste_goes_to_the_pointer_or_the_centre_of_the_view() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut state = CanvasInput::default();
    let key = Key::Character('v');
    let paste = Event::Key(KeyEvent::Pressed {
        key,
        modifiers: command(),
        text: None,
        repeat: false,
    });
    let at = Point::new(100.0, 300.0);
    let events = drive(&canvas, &mut state, &paste, at);
    assert_eq!(
        events,
        vec![CanvasEvent::Paste {
            world: state.camera.to_world(at)
        }]
    );
    let events = drive(&canvas, &mut state, &paste, Point::new(900.0, 700.0));
    assert_eq!(
        events,
        vec![CanvasEvent::Paste {
            world: state.camera.to_world(Point::new(400.0, 300.0))
        }]
    );
    // While a pop-up has the keys, paste is the pop-up's.
    let blocked = CanvasModel::new(&scene, &none).focus_policy(CanvasFocusPolicy {
        interactive: false,
        ..CanvasFocusPolicy::default()
    });
    assert!(drive(&blocked, &mut state, &paste, at).is_empty());
}

/// Shift pressed while a pop-up has the keys still extends the
/// selection on the first click after it closes.
#[test]
fn modifiers_stay_current_while_the_canvas_is_not_interactive() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let tile = scene.module(2).unwrap().bounds().center();
    let shift = Event::Key(KeyEvent::ModifiersChanged(SHIFT));
    let mut state = CanvasInput::default();

    let blocked = CanvasModel::new(&scene, &selected).focus_policy(CanvasFocusPolicy {
        interactive: false,
        ..CanvasFocusPolicy::default()
    });
    assert!(drive(&blocked, &mut state, &shift, tile).is_empty());
    assert!(state.modifiers.shift);

    let canvas = CanvasModel::new(&scene, &selected);
    let events = drive(&canvas, &mut state, &click(PointerButton::Left), tile);
    assert!(events.contains(&CanvasEvent::Select(vec![1, 2])), "{events:?}");
}

/// Pointer down reports a press and does not reset on a second press.
/// The app turns the press into a flip or a hold.
#[test]
fn a_button_reports_press_and_a_second_press_is_not_a_reset() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    let first = drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    assert_eq!(first, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);
    assert!(matches!(
        state.interaction,
        Interaction::Holding { module: 1, widget: 0 }
    ));

    let second = drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    assert_eq!(
        second,
        vec![
            CanvasEvent::ButtonReleased { module: 1, widget: 0 },
            CanvasEvent::ButtonPressed { module: 1, widget: 0 },
        ]
    );
    assert!(
        second
            .iter()
            .all(|event| !matches!(event, CanvasEvent::ResetControl { .. }))
    );
}

/// Release is reported even after the pointer has left the button.
/// Dragging does not move the tile.
#[test]
fn a_button_reports_release_outside() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let outside = Point::new(centre.x + 400.0, centre.y);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();

    let down = drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    assert_eq!(down, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);

    let moved = drive(&canvas, &mut state, &Event::Pointer(PointerEvent::Moved), outside);
    assert!(moved.iter().all(|event| !matches!(event, CanvasEvent::Move { .. })));

    let up = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        outside,
    );
    assert_eq!(up, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Idle));
}

/// Another mouse button ends the hold. Leaving the interaction as a pan
/// without a release would stick a momentary gate high.
#[test]
fn another_mouse_button_releases_a_hold() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);

    let right = drive(&canvas, &mut state, &click(PointerButton::Right), centre);
    assert_eq!(right, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Panning { .. }));
}

/// Losing the window while a key or the pointer is held reports a release.
/// The key-up event never arrives after the window is unfocused.
#[test]
fn losing_the_window_releases_a_hold() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        interaction: Interaction::Holding { module: 1, widget: 0 },
        key_holds: [
            Some(KeyHold {
                key: Key::Space,
                module: 1,
                widget: 0,
            }),
            None,
        ],
        ..CanvasInput::default()
    };
    let events = drive(&canvas, &mut state, &Event::WindowUnfocused, centre);
    assert_eq!(
        events,
        vec![
            CanvasEvent::ButtonReleased { module: 1, widget: 0 },
            CanvasEvent::EditGroupEnd
        ]
    );
    assert!(matches!(state.interaction, Interaction::Idle));
    assert!(state.key_holds.iter().all(Option::is_none));
}

fn key_event(named: Key, pressed: bool) -> Event {
    let key = named;
    if pressed {
        Event::Key(KeyEvent::Pressed {
            key,
            modifiers: Modifiers::default(),
            text: None,
            repeat: false,
        })
    } else {
        Event::Key(KeyEvent::Released { key })
    }
}

/// Space on one button and Enter on another. Releasing one key releases
/// only the button that key was holding.
#[test]
fn each_key_releases_only_its_own_hold() {
    let mut module = button_module();
    module.panel.width = 12;
    module.panel.widgets.push(widget(WidgetViewKind::Button, 6));
    let scene = Scene::new(vec![module], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let at = Point::new(20.0, 20.0);

    let space = drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert_eq!(space, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);

    state.focus.set(1, 1, false);
    let enter = drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    assert_eq!(enter, vec![CanvasEvent::ButtonPressed { module: 1, widget: 1 }]);

    let up_space = drive(&canvas, &mut state, &key_event(Key::Space, false), at);
    assert_eq!(up_space, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);

    let up_enter = drive(&canvas, &mut state, &key_event(Key::Enter, false), at);
    assert_eq!(up_enter, vec![CanvasEvent::ButtonReleased { module: 1, widget: 1 }]);
}

/// Both keys can hold one button. Releasing the first leaves it held.
#[test]
fn releasing_one_key_keeps_a_button_the_other_key_holds() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let at = Point::new(20.0, 20.0);
    drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    drive(&canvas, &mut state, &key_event(Key::Enter, true), at);

    let up_space = drive(&canvas, &mut state, &key_event(Key::Space, false), at);
    assert!(up_space.is_empty());

    let up_enter = drive(&canvas, &mut state, &key_event(Key::Enter, false), at);
    assert_eq!(up_enter, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
}

/// A right-button release does not end a hold the left button started.
#[test]
fn releasing_another_mouse_button_keeps_the_left_hold() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);

    let right = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Right)),
        centre,
    );
    assert!(right.is_empty());
    assert!(matches!(
        state.interaction,
        Interaction::Holding { module: 1, widget: 0 }
    ));
}

/// The gate stays down only when neither the pointer nor a key is holding.
#[test]
fn a_key_hold_survives_pointer_up_and_the_pointer_survives_a_key_up() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    drive(&canvas, &mut state, &key_event(Key::Space, true), centre);

    let pointer_up = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        centre,
    );
    assert!(pointer_up.is_empty());
    assert!(matches!(state.interaction, Interaction::Idle));

    state.interaction = Interaction::Holding { module: 1, widget: 0 };
    let key_up = drive(&canvas, &mut state, &key_event(Key::Space, false), centre);
    assert!(key_up.is_empty());
    assert!(matches!(
        state.interaction,
        Interaction::Holding { module: 1, widget: 0 }
    ));

    let pointer_up = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        centre,
    );
    assert_eq!(pointer_up, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
}

/// Space with no focused control asks the app to toggle Run. Enter does
/// not, a repeat does not, and a focused button still takes Space.
#[test]
fn space_with_no_focus_toggles_run() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let at = Point::new(10.0, 10.0);
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();

    let space = drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert_eq!(space, vec![CanvasEvent::ToggleRun]);

    let mut repeat = key_event(Key::Space, true);
    if let Event::Key(KeyEvent::Pressed { repeat, .. }) = &mut repeat {
        *repeat = true;
    }
    let again = drive(&canvas, &mut state, &repeat, at);
    assert!(again.is_empty());

    let enter = drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    assert!(enter.is_empty());

    state.focus.set(1, 0, false);
    let focused = drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert_eq!(focused, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);
}

fn learning<'a>(scene: &'a Scene, selected: &'a BTreeSet<u32>, learn: &'a LearnView) -> CanvasModel<'a> {
    CanvasModel::new(scene, selected).learn(Some(learn))
}

fn arrow_up() -> Event {
    let key = Key::Up;
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers: Modifiers::default(),
        text: None,
        repeat: false,
    })
}

/// A key event with Shift held, for reverse focus and coarse movement.
fn shifted(named: Key) -> Event {
    let mut event = key_event(named, true);
    if let Event::Key(KeyEvent::Pressed { modifiers, .. }) = &mut event {
        *modifiers = SHIFT;
    }
    event
}

/// Tab follows reading order across tiles and skips lights within a panel.
#[test]
fn keyboard_focus_follows_tiles_and_controls() {
    let mut first = module(1, 300.0);
    first.panel = panel();
    first.position.y = 100.0;
    let mut second = module(2, 0.0);
    second.panel = panel();
    let scene = Scene::new(vec![first, second], Vec::new());
    let at = Point::new(700.0, 500.0);
    let mut state = CanvasInput::default();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    assert_eq!(
        drive(&canvas, &mut state, &key_event(Key::Tab, true), at),
        vec![CanvasEvent::Select(vec![2])]
    );
    assert_eq!(
        drive(&canvas, &mut state, &shifted(Key::Tab), at),
        vec![CanvasEvent::Select(vec![1])],
        "Shift-Tab starts at the last tile"
    );
    let selected = BTreeSet::from([2]);
    let canvas = CanvasModel::new(&scene, &selected);
    assert_eq!(
        drive(&canvas, &mut state, &key_event(Key::Tab, true), at),
        vec![CanvasEvent::Select(vec![1])]
    );
    let first_selected = BTreeSet::from([1]);
    let first_canvas = CanvasModel::new(&scene, &first_selected);
    assert_eq!(
        drive(&first_canvas, &mut state, &shifted(Key::Tab), at),
        vec![CanvasEvent::Select(vec![2])]
    );
    assert!(drive(&canvas, &mut state, &key_event(Key::Enter, true), at).is_empty());
    assert_eq!(state.focus.control(), Some((2, 0)));
    assert!(drive(&canvas, &mut state, &key_event(Key::Tab, true), at).is_empty());
    assert_eq!(state.focus.control(), Some((2, 2)), "the light is skipped");
    assert!(drive(&canvas, &mut state, &shifted(Key::Tab), at).is_empty());
    assert_eq!(state.focus.control(), Some((2, 0)));
    let toggle = drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert_eq!(
        toggle,
        vec![CanvasEvent::ToggleRun],
        "a focused knob does not use Space"
    );
}

/// A fixed time for arrow events, so tests can compare them. See
/// [`untimed`].
fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// The events with every arrow event's time set to [`epoch`].
fn untimed(events: Vec<CanvasEvent>) -> Vec<CanvasEvent> {
    events
        .into_iter()
        .map(|event| match event {
            CanvasEvent::ArrowStep { .. } => CanvasEvent::ArrowStep { at: epoch() },
            CanvasEvent::ArrowPressed { .. } => CanvasEvent::ArrowPressed { at: epoch() },
            CanvasEvent::ArrowReleased { .. } => CanvasEvent::ArrowReleased { at: epoch() },
            CanvasEvent::EditGroupDue { .. } => CanvasEvent::EditGroupDue { at: epoch() },
            other => other,
        })
        .collect()
}

/// The group as the app shows it after a release, with a deadline far off.
fn released_group() -> EditGroupView {
    EditGroupView::Open {
        due: Instant::now() + Duration::from_secs(60),
    }
}

/// Shift pressed alone after an arrow release keeps the group open, so
/// Shift and the next arrow join the same step. Another key ends it.
#[test]
fn a_modifier_alone_keeps_a_released_group() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let released = CanvasModel::new(&scene, &selected).edit_group(released_group());
    let mut state = CanvasInput::default();
    let at = Point::new(700.0, 500.0);
    // Which modifier key it was is not told apart here: every one is
    // `Key::Modifier`. The conversion's own tests cover each of them.
    let events = drive(&released, &mut state, &key_event(Key::Modifier, true), at);
    assert!(
        !events.contains(&CanvasEvent::EditGroupEnd),
        "a modifier alone ended the group: {events:?}"
    );
    let other = drive(&released, &mut state, &key_event(Key::Tab, true), at);
    assert!(other.contains(&CanvasEvent::EditGroupEnd), "{other:?}");
}

/// An unfocused selected tile moves by one grid cell, or ten with Shift.
/// The canvas reports each arrow step and release; with the group released,
/// an arrow press says so first, and the next other key ends the group.
#[test]
fn keyboard_moves_selected_tile_and_closes_group_on_next_key() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let closed = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    let at = Point::new(700.0, 500.0);
    let first = untimed(drive(&closed, &mut state, &arrow_up(), at));
    assert_eq!(
        first,
        vec![
            CanvasEvent::ArrowStep { at: epoch() },
            CanvasEvent::Move {
                ids: vec![1],
                delta: Vector::new(0.0, -GRID)
            }
        ]
    );
    let up = key_event(Key::Up, false);
    assert_eq!(
        untimed(drive(&closed, &mut state, &up, at)),
        vec![CanvasEvent::ArrowReleased { at: epoch() }]
    );

    let released = CanvasModel::new(&scene, &selected).edit_group(released_group());
    let next = untimed(drive(&released, &mut state, &shifted(Key::Right), at));
    assert_eq!(
        next,
        vec![
            CanvasEvent::ArrowPressed { at: epoch() },
            CanvasEvent::ArrowStep { at: epoch() },
            CanvasEvent::Move {
                ids: vec![1],
                delta: Vector::new(10.0 * GRID, 0.0)
            }
        ]
    );
    assert_eq!(
        drive(&released, &mut state, &escape(), at),
        vec![CanvasEvent::EditGroupEnd, CanvasEvent::Select(Vec::new())]
    );
    let plain = drive(&closed, &mut state, &escape(), at);
    assert!(
        !plain.contains(&CanvasEvent::EditGroupEnd),
        "with no group open, Escape ends none: {plain:?}"
    );
}

/// A pointer press ends an open arrow group before starting its own
/// interaction, so its eventual release cannot close the arrow step. With
/// no group open it sends no end.
#[test]
fn pointer_press_ends_an_open_arrow_group() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let at = Point::new(700.0, 500.0);
    for group in [EditGroupView::Held, released_group()] {
        let canvas = CanvasModel::new(&scene, &selected).edit_group(group);
        let press = drive(&canvas, &mut CanvasInput::default(), &click(PointerButton::Left), at);
        assert_eq!(press.first(), Some(&CanvasEvent::EditGroupEnd), "{group:?}: {press:?}");
    }
    let canvas = CanvasModel::new(&scene, &selected);
    let press = drive(&canvas, &mut CanvasInput::default(), &click(PointerButton::Left), at);
    assert!(!press.contains(&CanvasEvent::EditGroupEnd), "{press:?}");
}

/// Losing the window ends the group whatever the view shows, because no key
/// release will come to end it.
#[test]
fn losing_the_window_always_ends_the_arrow_group() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let lost = drive(
        &canvas,
        &mut CanvasInput::default(),
        &Event::WindowUnfocused,
        Point::ORIGIN,
    );
    assert!(lost.contains(&CanvasEvent::EditGroupEnd), "{lost:?}");
}

/// Keys that belong to an app text field leave the canvas, and end an open
/// group, but send nothing with none open.
#[test]
fn keys_leaving_the_canvas_end_only_an_open_group() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let policy = CanvasFocusPolicy {
        text_field_keys: true,
        ..CanvasFocusPolicy::default()
    };
    let at = Point::new(700.0, 500.0);
    let quiet = CanvasModel::new(&scene, &selected).focus_policy(policy);
    assert!(drive(&quiet, &mut CanvasInput::default(), &arrow_up(), at).is_empty());
    let open = CanvasModel::new(&scene, &selected)
        .focus_policy(policy)
        .edit_group(released_group());
    assert_eq!(
        drive(&open, &mut CanvasInput::default(), &arrow_up(), at),
        vec![CanvasEvent::EditGroupEnd]
    );
}

/// Keyboard navigation selects a tile, enters its control, edits it,
/// and leaves both focus levels without a pointer gesture (#101).
#[test]
fn keys_reach_and_leave_a_panel_control() {
    let mut first = module(1, 0.0);
    first.panel = panel();
    let scene = Scene::new(vec![first], Vec::new());
    let none = BTreeSet::new();
    let mut state = CanvasInput::default();
    let at = Point::new(700.0, 500.0);
    let canvas = CanvasModel::new(&scene, &none);
    assert_eq!(
        drive(&canvas, &mut state, &key_event(Key::Tab, true), at),
        vec![CanvasEvent::Select(vec![1])]
    );
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    assert!(drive(&canvas, &mut state, &key_event(Key::Enter, true), at).is_empty());
    assert_eq!(state.focus.control(), Some((1, 0)));
    let changed = drive(&canvas, &mut state, &arrow_up(), at);
    assert!(
        changed.iter().any(|e| matches!(
            e,
            CanvasEvent::SetControl {
                module: 1,
                widget: 0,
                ..
            }
        )),
        "{changed:?}"
    );
    drive(&canvas, &mut state, &key_event(Key::Up, false), at);
    // The app now shows the group released, so Escape ends it first.
    let released = CanvasModel::new(&scene, &selected).edit_group(released_group());
    assert_eq!(
        drive(&released, &mut state, &escape(), at),
        vec![CanvasEvent::EditGroupEnd]
    );
    assert_eq!(state.focus.control(), None);
    assert_eq!(
        drive(&canvas, &mut state, &escape(), at),
        vec![CanvasEvent::Select(Vec::new())]
    );
}

/// In MIDI learn mode a click arms a knob or a button and takes the
/// keyboard focus. It never turns the knob or presses the button.
#[test]
fn a_learn_click_arms_and_focuses_without_changing_a_value() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let button = ModuleView {
        id: 2,
        position: Point::new(400.0, 0.0),
        ..button_module()
    };
    let scene = Scene::new(vec![m, button], Vec::new());
    let selected = BTreeSet::from([1]);
    let learn = LearnView::default();
    let canvas = learning(&scene, &selected, &learn);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 2, true, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let knob = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let events = drive(&canvas, &mut state, &click(PointerButton::Left), knob);
    assert_eq!(events, vec![CanvasEvent::Learn { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Idle));
    assert_eq!(state.focus.control(), Some((1, 0)));

    let button = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    let events = drive(&canvas, &mut state, &click(PointerButton::Left), button);
    assert!(
        events.contains(&CanvasEvent::Learn { module: 2, widget: 0 }),
        "{events:?}"
    );
    assert!(
        events.iter().all(|e| !matches!(e, CanvasEvent::ButtonPressed { .. })),
        "{events:?}"
    );
}

/// Outside learn mode, a click on a control only changes its value: it never
/// asks to arm anything, so moving a hardware control next maps nothing
/// (midi-learn.md, Acceptance criteria).
#[test]
fn outside_learn_mode_a_click_arms_nothing() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    let knob = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let events = drive(&canvas, &mut state, &click(PointerButton::Left), knob);
    assert!(
        events.iter().all(|e| !matches!(e, CanvasEvent::Learn { .. })),
        "{events:?}"
    );
}

/// In learn mode a mapped control's caption shows its mapping, and an
/// unmapped control, other text, or no learn mode shows none (midi-learn.md,
/// Acceptance criteria).
#[test]
fn a_mapped_control_shows_its_label_in_learn_mode() {
    let mut learn = LearnView::default();
    learn.labels.insert((1, 0), "CC 74".to_string());
    assert_eq!(
        overlay::learned_label(Some(&learn), 1, crate::tile_text::TextKind::Caption(0)),
        Some("CC 74")
    );
    assert_eq!(
        overlay::learned_label(Some(&learn), 1, crate::tile_text::TextKind::Caption(1)),
        None
    );
    assert_eq!(
        overlay::learned_label(Some(&learn), 2, crate::tile_text::TextKind::Caption(0)),
        None
    );
    assert_eq!(
        overlay::learned_label(None, 1, crate::tile_text::TextKind::Caption(0)),
        None
    );
    assert_eq!(
        overlay::learned_label(Some(&learn), 1, crate::tile_text::TextKind::Title),
        None
    );
}

/// In MIDI learn mode Space and Enter arm the focused control, arrows do
/// nothing, and Escape is left for the app, which ends the mode.
#[test]
fn learn_mode_keys_arm_and_never_change_a_value() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let learn = LearnView::default();
    let canvas = learning(&scene, &selected, &learn);
    let at = Point::new(10.0, 10.0);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, true, FocusMode::Browse),
        ..CanvasInput::default()
    };
    for key in [Key::Space, Key::Enter] {
        let events = drive(&canvas, &mut state, &key_event(key, true), at);
        assert_eq!(events, vec![CanvasEvent::Learn { module: 1, widget: 0 }]);
    }
    assert!(drive(&canvas, &mut state, &arrow_up(), at).is_empty());

    let escape = {
        let key = Key::Escape;
        Event::Key(KeyEvent::Pressed {
            key,
            modifiers: Modifiers::default(),
            text: None,
            repeat: false,
        })
    };
    let mut out = Reaction::default();
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    canvas.handle(&mut state, Some(&escape), Some(at), bounds, Instant::now(), &mut out);
    assert!(!out.taken, "the app must see Escape");
    assert_eq!(state.focus.control(), None);

    let mut next_out = Reaction::default();
    canvas.handle(
        &mut state,
        Some(&escape),
        Some(at),
        bounds,
        Instant::now(),
        &mut next_out,
    );
    assert!(
        !next_out.taken,
        "learn mode must receive Escape even with a selected tile"
    );

    // With nothing focused, Space is still the Run shortcut.
    let events = drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert_eq!(events, vec![CanvasEvent::ToggleRun]);
}

/// Learn mode outlines controls on screen only.
#[test]
fn learn_outlines_cover_only_visible_tiles() {
    let mut near = module(1, 0.0);
    near.panel = panel();
    let far = ModuleView {
        panel: panel(),
        ..module(2, 5_000.0)
    };
    let scene = Scene::new(vec![near, far], Vec::new());
    let none = BTreeSet::new();
    let learn = LearnView::default();
    let canvas = learning(&scene, &none, &learn);
    let plain = canvas
        .overlay(&CanvasInput::default(), &LinesCache::default())
        .shapes
        .len();
    let view = Rect::new(Point::new(-50.0, -50.0), Size::new(800.0, 600.0));
    // Two controls (a knob and a toggle) on the one visible tile.
    assert_eq!(
        canvas
            .overlay_in(&CanvasInput::default(), &LinesCache::default(), view)
            .shapes
            .len(),
        plain + 2
    );
}

fn knob_scene() -> (Scene, Point) {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let knob = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    (scene, knob)
}

/// A pointer move. The place travels beside the event, so `_to` is only
/// here to say where the caller means.
fn moved(_to: Point) -> Event {
    Event::Pointer(PointerEvent::Moved)
}

/// A source tile with one output, and a tile with a knob, a light, and
/// a toggle further right.
fn assign_scene() -> Scene {
    let target = ModuleView {
        panel: panel(),
        ..module(2, 300.0)
    };
    Scene::new(vec![module(1, 0.0), target], Vec::new())
}

const OUT: PortRef = PortRef {
    module: 1,
    output: true,
    index: 0,
};

/// Drags from the source output to widget `widget` on tile 2 and lets
/// go there. Returns the events and the overlay shape count while over
/// the widget.
fn drop_on(canvas: &CanvasModel<'_>, scene: &Scene, widget: usize) -> (Vec<CanvasEvent>, usize) {
    let mut state = CanvasInput::default();
    let start = scene.port_position(OUT).unwrap();
    drive(canvas, &mut state, &click(PointerButton::Left), start);
    let over = scene.module(2).unwrap().widget_bounds(widget).unwrap().center();
    drive(canvas, &mut state, &moved(over), over);
    let shapes = canvas.overlay(&state, &LinesCache::default()).shapes.len();
    (drive(canvas, &mut state, &released(), over), shapes)
}

/// Dropping an output on a knob or toggle assigns it, and the control is
/// highlighted while the drag is over it. A light takes nothing
/// (modulation.md, Making an assignment).
#[test]
fn dropping_an_output_on_a_control_assigns_it() {
    let scene = assign_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let (events, on_knob) = drop_on(&canvas, &scene, 0);
    assert_eq!(
        events,
        vec![
            CanvasEvent::Assign {
                from: OUT,
                module: 2,
                widget: 0,
                part: ControlPart::Main
            },
            CanvasEvent::AssignCancel,
            CanvasEvent::GestureEnd
        ]
    );
    let (events, _) = drop_on(&canvas, &scene, 2);
    assert!(events.contains(&CanvasEvent::Assign {
        from: OUT,
        module: 2,
        widget: 2,
        part: ControlPart::Main
    }));
    let (events, on_light) = drop_on(&canvas, &scene, 1);
    assert_eq!(events, vec![CanvasEvent::AssignCancel, CanvasEvent::GestureEnd]);
    assert_eq!(
        on_knob,
        on_light + 1,
        "only a control that takes the drop is highlighted"
    );
}

/// In MIDI learn mode a drop on a control makes no assignment, and
/// nothing is highlighted.
#[test]
fn learn_mode_takes_no_assignment_drop() {
    let scene = assign_scene();
    let none = BTreeSet::new();
    let learn = LearnView::default();
    let canvas = learning(&scene, &none, &learn);
    let (events, _) = drop_on(&canvas, &scene, 0);
    assert_eq!(events, vec![CanvasEvent::AssignCancel, CanvasEvent::GestureEnd]);
}

/// The assign scene with one assignment from the source output to the
/// knob on tile 2, and the knob showing its range.
fn modulated_scene() -> (Scene, PanelValues) {
    let mut source = module(1, 0.0);
    source.outputs[0].badge = 1;
    let target = ModuleView {
        panel: panel(),
        ..module(2, 300.0)
    };
    let scene = Scene::new(vec![source, target], Vec::new()).with_modulations(vec![crate::scene::ModulationView {
        id: 1,
        from: (1, 0),
        to: 2,
        widget: Some(0),
        enabled: true,
    }]);
    let mut values = PanelValues::new();
    values.set(
        2,
        0,
        crate::controls::WidgetValue {
            position: 0.5,
            modulation: crate::controls::ModulationMark::Range { low: 0.5, high: 0.75 },
            ..Default::default()
        },
    );
    (scene, values)
}

/// #249: an output's hover text is placed above its badge as well as
/// its port, so the box never covers the count. An input's stays on
/// its port.
#[test]
fn output_hover_text_goes_above_its_badge() {
    let (scene, _) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let out = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let badge = scene.badge_bounds(1, 0).unwrap();
    let (_, _, _, anchor) = canvas.hover_content(HoverTarget::Port(out)).unwrap();
    assert!(
        anchor.y <= badge.y && anchor.x + anchor.width >= badge.x + badge.width,
        "{anchor:?}"
    );
    let input = PortRef { output: false, ..out };
    let (_, _, _, anchor) = canvas.hover_content(HoverTarget::Port(input)).unwrap();
    assert!(anchor.width <= 2.0 * PORT_RADIUS + 1e-3, "{anchor:?}");
}

fn escape() -> Event {
    let key = Key::Escape;
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers: Modifiers::default(),
        text: None,
        repeat: false,
    })
}

/// A still image starts with the pointer, pin, drag, and hover text it
/// asks for, all in the first frame (`loom-app --screenshot`).
#[test]
fn a_still_seeds_pointer_pin_drag_and_hover_text() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    let still = Still {
        pointer: Some(knob),
        pinned: Some(OUT),
        hover_text: true,
        ..Still::default()
    };
    let canvas = CanvasModel::new(&scene, &none).values(&values).still(still);
    let state = canvas.still_state(still, Instant::now());
    assert_eq!(state.hover, Some(Hit::Widget { module: 2, widget: 0 }));
    assert_eq!(state.pinned, Some(OUT));
    assert_eq!(state.hover_text.shown(), Some(HoverTarget::Control(2, 0)));
    assert_eq!(
        canvas.overlay(&state, &LinesCache::default()).cables.len(),
        1,
        "the modulation line"
    );

    // Wiring from the output to the knob: the loose cable and the drop
    // highlight, and no hover text during a drag.
    let wiring = Still {
        drag: Some(StillDrag::Wire {
            from: OUT,
            pulled: false,
        }),
        pinned: None,
        ..still
    };
    let state = canvas.still_state(wiring, Instant::now());
    assert!(matches!(
        state.interaction,
        Interaction::Wiring {
            from: OUT,
            pulled: false
        }
    ));
    assert_eq!(state.hover_text.shown(), None);
    let overlay = canvas.overlay(&state, &LinesCache::default());
    assert_eq!(overlay.cables.len(), 2, "the line and the loose cable");
    let resting = canvas.overlay(
        &canvas.still_state(Still { drag: None, ..wiring }, Instant::now()),
        &LinesCache::default(),
    );
    assert_eq!(overlay.shapes.len(), resting.shapes.len() + 1, "the drop highlight");

    // A cable pulled off an input assigns nothing on a drop, so the
    // control is not highlighted.
    let pulled = canvas.overlay(
        &canvas.still_state(
            Still {
                drag: Some(StillDrag::Wire {
                    from: OUT,
                    pulled: true,
                }),
                ..wiring
            },
            Instant::now(),
        ),
        &LinesCache::default(),
    );
    assert_eq!(pulled.shapes.len(), resting.shapes.len());
}

/// Lines show while the pointer is on the source output, its badge, or
/// the modulated control, and not elsewhere. A badge click pins them;
/// a second click or Escape unpins, and Escape is used up only then.
#[test]
fn modulation_lines_follow_hover_and_the_pinned_badge() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    let lines = |state: &CanvasInput| canvas.overlay(state, &LinesCache::default()).cables.len();
    let badge = scene.badge_bounds(1, 0).unwrap().center();
    assert_eq!(scene.hit(badge, PORT_HIT_RADIUS), Hit::Badge(OUT));
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    let port = scene.port_position(OUT).unwrap();
    let empty = Point::new(200.0, 400.0);

    let mut state = CanvasInput::default();
    for (at, shown) in [(port, 1), (badge, 1), (knob, 1), (empty, 0)] {
        drive(&canvas, &mut state, &moved(at), at);
        assert_eq!(lines(&state), shown, "pointer at {at:?}");
    }

    drive(&canvas, &mut state, &moved(badge), badge);
    assert!(drive(&canvas, &mut state, &click(PointerButton::Left), badge).is_empty());
    drive(&canvas, &mut state, &released(), badge);
    drive(&canvas, &mut state, &moved(empty), empty);
    assert_eq!(state.pinned, Some(OUT));
    assert_eq!(lines(&state), 1, "pinned lines stay");

    let mut out = Reaction::default();
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    canvas.handle(
        &mut state,
        Some(&escape()),
        Some(empty),
        bounds,
        Instant::now(),
        &mut out,
    );
    assert!(out.taken);
    assert_eq!(state.pinned, None);
    assert_eq!(lines(&state), 0);
    let mut out = Reaction::default();
    canvas.handle(
        &mut state,
        Some(&escape()),
        Some(empty),
        bounds,
        Instant::now(),
        &mut out,
    );
    assert!(!out.taken, "with nothing pinned the app sees Escape");

    // Two clicks on the badge pin and unpin.
    drive(&canvas, &mut state, &click(PointerButton::Left), badge);
    drive(&canvas, &mut state, &click(PointerButton::Left), badge);
    assert_eq!(state.pinned, None);
}

/// A pointed source has two rings and its target one; selection alone
/// still draws lines without outlines (#407).
#[test]
fn modulation_outlines_show_direction_only_for_pointing() {
    let (scene, _) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut pointed = Overlay::default();
    canvas.build_modulation_lines(
        &CanvasInput {
            hover: Some(Hit::Port(OUT)),
            ..CanvasInput::default()
        },
        &mut pointed,
    );
    assert_eq!(pointed.cables.len(), 1);
    assert_eq!(pointed.shapes.len(), 3, "two source rings and one target ring");

    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut selected_only = Overlay::default();
    canvas.build_modulation_lines(&CanvasInput::default(), &mut selected_only);
    assert_eq!(selected_only.cables.len(), 1);
    assert!(selected_only.shapes.is_empty());
}

/// Turning an assignment off keeps its dashed line and direction rings,
/// while the line uses the disabled style (#197).
#[test]
fn disabled_assignment_keeps_a_visible_line() {
    let (mut scene, _) = modulated_scene();
    scene.set_modulation_enabled(1, false);
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut overlay = Overlay::default();
    canvas.build_modulation_lines(
        &CanvasInput {
            hover: Some(Hit::Port(OUT)),
            ..CanvasInput::default()
        },
        &mut overlay,
    );
    assert_eq!(overlay.cables.len(), 1);
    assert_eq!(overlay.shapes.len(), 3);
}

/// Toggling one assignment updates its line cache without changing
/// the scene generation used for tiles and cables (#197).
#[test]
fn assignment_state_updates_only_modulation_overlay() {
    let (mut scene, _) = modulated_scene();
    let geometry = scene.generation();
    let lines = scene.overlay_generation();
    scene.set_modulation_enabled(1, false);
    assert_eq!(scene.generation(), geometry);
    assert_ne!(scene.overlay_generation(), lines);
    assert!(!scene.modulations()[0].enabled);
}

/// A drag on the ring around a modulated knob changes depth, up to
/// raise it, as one gesture, and carries the pinned output. A drag on
/// the knob body still turns it.
#[test]
fn dragging_the_range_ring_changes_depth() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    let rect = scene.module(2).unwrap().widget_bounds(0).unwrap();
    let centre = crate::controls::knob_centre(rect);
    let ring = Point::new(centre.x, centre.y - 12.0);

    let mut state = CanvasInput {
        pinned: Some(OUT),
        ..CanvasInput::default()
    };
    let down = drive(&canvas, &mut state, &click(PointerButton::Left), ring);
    assert!(down.contains(&CanvasEvent::GestureStart), "{down:?}");
    // The selection stays: a selected source picks the assignment.
    assert!(!down.iter().any(|e| matches!(e, CanvasEvent::Select(_))), "{down:?}");
    let up = Point::new(ring.x, ring.y - 20.0);
    let moved_events = drive(&canvas, &mut state, &moved(up), up);
    let [
        CanvasEvent::DragDepth {
            module: 2,
            widget: 0,
            pinned: Some(pinned),
            delta,
            part: ControlPart::Main,
        },
    ] = moved_events.as_slice()
    else {
        panic!("{moved_events:?}");
    };
    assert_eq!(*pinned, OUT);
    assert!((delta - 0.1).abs() < 1e-6, "{delta}");
    assert_eq!(
        drive(&canvas, &mut state, &released(), up),
        vec![CanvasEvent::GestureEnd]
    );

    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), centre);
    let up = Point::new(centre.x, centre.y - 20.0);
    let turned = drive(&canvas, &mut state, &moved(up), up);
    assert!(
        matches!(turned.as_slice(), [CanvasEvent::SetControl { .. }]),
        "{turned:?}"
    );
}

/// A pin whose badge is gone (its last assignment removed) counts as
/// none: Escape goes to the app, and an arc drag carries no pin.
#[test]
fn a_pin_without_its_badge_takes_no_escape() {
    let (mut scene, values) = modulated_scene();
    let modules = scene.modules().to_vec();
    let mut source = modules[0].clone();
    source.outputs[0].badge = 0;
    scene = Scene::new(vec![source, modules[1].clone()], Vec::new());
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    let mut state = CanvasInput {
        pinned: Some(OUT),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.pin(&state), None);
    let mut out = Reaction::default();
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    canvas.handle(
        &mut state,
        Some(&escape()),
        Some(Point::ORIGIN),
        bounds,
        Instant::now(),
        &mut out,
    );
    assert!(!out.taken);
}

/// A new patch drops the pin. Module IDs repeat between patches. The arrow
/// group needs nothing here: the app owns it, and a new patch closes it.
#[test]
fn a_new_patch_clears_old_canvas_state() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values).frame_key(2);
    let mut state = CanvasInput {
        pinned: Some(OUT),
        framed_key: Some(1),
        ..CanvasInput::default()
    };
    let redraw = Event::Redraw;
    let events = drive(&canvas, &mut state, &redraw, Point::ORIGIN);
    assert!(!events.contains(&CanvasEvent::EditGroupEnd), "{events:?}");
    assert_eq!(state.pinned, None);
}

/// A redraw at or past the deadline the app gave says so, with its time,
/// and one before it says nothing.
#[test]
fn a_redraw_past_the_deadline_reports_it() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let due = Instant::now() + Duration::from_millis(100);
    let canvas = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Open { due });
    let mut state = CanvasInput::default();
    let early = drive_at(
        &canvas,
        &mut state,
        Some(&Event::Redraw),
        Point::ORIGIN,
        due - Duration::from_millis(1),
    );
    assert!(
        !early.iter().any(|e| matches!(e, CanvasEvent::EditGroupDue { .. })),
        "{early:?}"
    );
    let late = drive_at(&canvas, &mut state, Some(&Event::Redraw), Point::ORIGIN, due);
    assert!(late.contains(&CanvasEvent::EditGroupDue { at: due }), "{late:?}");
}

/// While the group is released, every event asks for a redraw at its
/// deadline, not only the first, so the deadline is never lost on an idle
/// patch: the runtime drops a pending one when a later frame is asked for.
/// Within one event the earlier request wins, so the events here ask for no
/// frame of their own.
#[test]
fn every_event_asks_for_a_redraw_at_the_deadline() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let due = Instant::now() + Duration::from_millis(400);
    let canvas = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Open { due });
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    // Already framed and reported, so a redraw asks for nothing itself.
    let mut state = CanvasInput::default();
    let at = Point::new(100.0, 100.0);
    let redraw = Event::Redraw;
    canvas.on_event(
        &mut state,
        Some(&redraw),
        Some(at),
        bounds,
        Instant::now(),
        &mut Reaction::default(),
    );
    for event in [redraw, Event::Key(KeyEvent::ModifiersChanged(SHIFT))] {
        let mut out = Reaction::default();
        canvas.on_event(&mut state, Some(&event), Some(at), bounds, Instant::now(), &mut out);
        assert!(!out.redraw, "{event:?} asks for no frame of its own");
        assert_eq!(state.next_deadline(&canvas), Some(due), "{event:?}");
    }
    let closed = CanvasModel::new(&scene, &none);
    let mut out = Reaction::default();
    let shift = Event::Key(KeyEvent::ModifiersChanged(SHIFT));
    closed.on_event(&mut state, Some(&shift), Some(at), bounds, Instant::now(), &mut out);
    assert_ne!(state.next_deadline(&closed), Some(due), "no group, no wake");
}

/// Something the window reports that means nothing to the canvas, such as
/// the window moving, still asks for the redraw at the deadline. Without
/// this the deadline could wait for some later event.
#[test]
fn an_event_with_no_meaning_still_asks_for_a_redraw_at_the_deadline() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let due = Instant::now() + Duration::from_millis(400);
    let canvas = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Open { due });
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut state = CanvasInput::default();
    let mut out = Reaction::default();
    canvas.on_event(&mut state, None, None, bounds, Instant::now(), &mut out);
    assert_eq!(state.next_deadline(&canvas), Some(due));
    assert!(!out.redraw);
    assert!(!out.taken);
    assert!(out.events.is_empty(), "{:?}", out.events);
}

/// The lines are built once for a given pin, pointer, and selection,
/// and reused while those stay the same.
#[test]
fn modulation_lines_are_reused_until_their_inputs_change() {
    let (scene, values) = modulated_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected).values(&values);
    let state = CanvasInput::default();
    let lines = LinesCache::default();
    let first = canvas.overlay(&state, &lines).cables.len();
    let key = lines.0.borrow().as_ref().map(|(k, _)| k.clone());
    assert_eq!(canvas.overlay(&state, &lines).cables.len(), first);
    assert_eq!(lines.0.borrow().as_ref().map(|(k, _)| k.clone()), key);
    assert_eq!(first, 1, "the selected source's line");
}

/// A matrix-selected row draws its line with no hover, canvas selection,
/// or badge pin, and leaves the pin state alone.
#[test]
fn matrix_row_shows_one_line_without_pinning() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let plain = CanvasModel::new(&scene, &none).values(&values);
    assert!(
        plain
            .overlay(&CanvasInput::default(), &LinesCache::default())
            .cables
            .is_empty()
    );
    let line = scene.modulations()[0].clone();
    let canvas = CanvasModel::new(&scene, &none).values(&values).matrix_line(Some(line));
    let state = CanvasInput::default();
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 1);
    assert_eq!(state.pinned, None);
}

/// A matrix line adds to lines shown by a pinned or hovered output.
#[test]
fn matrix_line_preserves_pinned_and_hovered_lines() {
    let (original, values) = modulated_scene();
    let mut source = module(3, 600.0);
    source.outputs[0].badge = 1;
    let modules = original.modules();
    let selected = original.modulations()[0].clone();
    let pin = PortRef {
        module: 3,
        output: true,
        index: 0,
    };
    let scene = Scene::new(vec![modules[0].clone(), modules[1].clone(), source], Vec::new()).with_modulations(vec![
        selected.clone(),
        crate::scene::ModulationView {
            id: 2,
            from: (3, 0),
            to: 2,
            widget: Some(0),
            enabled: true,
        },
    ]);
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none)
        .values(&values)
        .matrix_line(Some(selected));
    let mut state = CanvasInput {
        pinned: Some(pin),
        ..CanvasInput::default()
    };
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 2);
    state.pinned = None;
    state.hover = Some(Hit::Port(pin));
    assert_eq!(canvas.overlay(&state, &LinesCache::default()).cables.len(), 2);
}

/// Escape can clear a canvas pin while matrix keyboard focus owns the key.
#[test]
fn matrix_escape_clears_canvas_pin() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none)
        .values(&values)
        .focus_policy(CanvasFocusPolicy {
            matrix_keyboard: true,
            matrix_escape: true,
            ..CanvasFocusPolicy::default()
        });
    let mut state = CanvasInput {
        pinned: Some(OUT),
        ..CanvasInput::default()
    };
    drive(&canvas, &mut state, &escape(), Point::ORIGIN);
    assert_eq!(state.pinned, None);
}

/// Matrix keyboard focus leaves canvas clicks active but owns ordinary keys.
#[test]
fn matrix_keyboard_focus_keeps_canvas_pointer_active() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let tile = scene.module(2).unwrap().bounds().center();
    let canvas = CanvasModel::new(&scene, &selected).focus_policy(CanvasFocusPolicy {
        matrix_keyboard: true,
        ..CanvasFocusPolicy::default()
    });
    let mut state = CanvasInput::default();

    assert!(drive(&canvas, &mut state, &key_event(Key::Space, true), tile).is_empty());
    let events = drive(&canvas, &mut state, &click(PointerButton::Left), tile);
    assert!(events.contains(&CanvasEvent::Select(vec![2])), "{events:?}");
}

/// A knob whose arc has zero length shows the dot, and its ring still
/// takes a depth drag, so depth 0 can be dragged back out of.
#[test]
fn a_zero_length_arc_still_takes_a_depth_drag() {
    let (scene, _) = modulated_scene();
    let mut values = PanelValues::new();
    values.set(
        2,
        0,
        crate::controls::WidgetValue {
            modulation: crate::controls::ModulationMark::Dot,
            ..Default::default()
        },
    );
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    let centre = crate::controls::knob_centre(scene.module(2).unwrap().widget_bounds(0).unwrap());
    let mut state = CanvasInput::default();
    drive(
        &canvas,
        &mut state,
        &click(PointerButton::Left),
        Point::new(centre.x + 12.0, centre.y),
    );
    assert!(matches!(state.interaction, Interaction::Depth { .. }));
}

/// Just above a modulated knob's cell, on its ring, the pointer shows
/// the depth drag cursor. Over an unmodulated knob there, and in MIDI
/// learn mode, it shows the tile's grab cursor, as a press there grabs
/// the tile (#198).
#[test]
fn the_range_ring_shows_the_resize_cursor_above_the_cell() {
    let (scene, values) = modulated_scene();
    let rect = scene.module(2).unwrap().widget_bounds(0).unwrap();
    // On the ring, just above the cell: the hit test finds the tile.
    let (centre, distance) = crate::controls::knob_range_ring(rect);
    let above = Point::new(centre.x, centre.y - distance);
    assert!(above.y < rect.y, "the point should be above the cell");
    let none = BTreeSet::new();
    let cursor_at = |canvas: &CanvasModel<'_>| {
        let mut state = CanvasInput::default();
        drive(canvas, &mut state, &moved(above), above);
        assert!(matches!(state.hover, Some(Hit::Module(2))), "{:?}", state.hover);
        canvas.hover_interaction(&state)
    };

    let modulated = CanvasModel::new(&scene, &none).values(&values);
    assert_eq!(cursor_at(&modulated), CursorShape::ResizeVertical);

    let plain_values = PanelValues::new();
    let plain = CanvasModel::new(&scene, &none).values(&plain_values);
    assert_eq!(cursor_at(&plain), CursorShape::Grab);

    let learn = LearnView::default();
    let learning = CanvasModel::new(&scene, &none).values(&values).learn(Some(&learn));
    assert_eq!(cursor_at(&learning), CursorShape::Grab);
}

/// A knob with no enabled assignment has no ring to grab: a press there
/// turns the knob.
#[test]
fn an_unmodulated_knob_has_no_range_ring() {
    let (scene, _) = modulated_scene();
    let values = PanelValues::new();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    let centre = crate::controls::knob_centre(scene.module(2).unwrap().widget_bounds(0).unwrap());
    // Beside the knob, inside its cell, where the ring would be.
    let ring = Point::new(centre.x + 12.0, centre.y);
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), ring);
    assert!(matches!(state.interaction, Interaction::Turning { .. }));
}

/// A cable pulled off a connected input and dropped on a control is
/// discarded. It does not become an assignment.
#[test]
fn a_pulled_cable_dropped_on_a_control_does_not_assign() {
    let cable = CableView {
        from: (1, 0),
        to: (2, 0),
        colour_from: Color::WHITE,
        colour_to: Color::WHITE,
        delayed: false,
    };
    let target = ModuleView {
        panel: panel(),
        ..module(2, 300.0)
    };
    let scene = Scene::new(vec![module(1, 0.0), target], vec![cable]);
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut state = CanvasInput::default();
    let input = PortRef {
        module: 2,
        output: false,
        index: 0,
    };
    let start = scene.port_position(input).unwrap();
    let down = drive(&canvas, &mut state, &click(PointerButton::Left), start);
    assert!(down.contains(&CanvasEvent::Disconnect(input)), "{down:?}");
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    drive(&canvas, &mut state, &moved(knob), knob);
    let up = drive(&canvas, &mut state, &released(), knob);
    assert_eq!(up, vec![CanvasEvent::GestureEnd]);
}

/// A drag that starts on an input never assigns.
#[test]
fn an_input_drag_does_not_assign() {
    let scene = assign_scene();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut state = CanvasInput::default();
    let input = PortRef { output: false, ..OUT };
    let start = scene.port_position(input).unwrap();
    drive(&canvas, &mut state, &click(PointerButton::Left), start);
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    drive(&canvas, &mut state, &moved(knob), knob);
    assert_eq!(
        drive(&canvas, &mut state, &released(), knob),
        vec![CanvasEvent::GestureEnd]
    );
}

fn released() -> Event {
    Event::Pointer(PointerEvent::Released(PointerButton::Left))
}

/// A knob drag is one gesture: start at pointer down, end at pointer up
/// (undo.md, Gestures).
#[test]
fn a_knob_drag_is_bracketed_by_a_gesture_start_and_end() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    let down = drive(&canvas, &mut state, &click(PointerButton::Left), knob);
    assert_eq!(down, vec![CanvasEvent::GestureStart]);
    let up = Point::new(knob.x, knob.y - 20.0);
    let during = drive(&canvas, &mut state, &moved(up), up);
    assert!(
        matches!(during.as_slice(), [CanvasEvent::SetControl { .. }]),
        "{during:?}"
    );
    let end = drive(&canvas, &mut state, &released(), up);
    assert_eq!(end, vec![CanvasEvent::GestureEnd]);
}

/// A drag cut off by the window losing focus still ends its gesture, once.
#[test]
fn losing_the_window_ends_a_drag_gesture() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), knob);
    let lost = drive(&canvas, &mut state, &Event::WindowUnfocused, knob);
    // The arrow group ends too: losing the window always sends its end.
    assert_eq!(lost, vec![CanvasEvent::GestureEnd, CanvasEvent::EditGroupEnd]);
    let later = drive(&canvas, &mut state, &released(), knob);
    assert!(later.is_empty(), "{later:?}");
}

/// A second button pressed in a drag replaces the interaction, and must
/// end the gesture first.
#[test]
fn another_button_during_a_drag_ends_the_gesture() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    drive(&canvas, &mut state, &click(PointerButton::Left), knob);
    let events = drive(&canvas, &mut state, &click(PointerButton::Right), knob);
    assert_eq!(events, vec![CanvasEvent::GestureEnd]);
}

/// A held arrow key repeats. Each press reports a step, and the app keeps
/// them in one group; key up reports the release (`docs/design/undo.md`).
#[test]
fn a_held_arrow_key_reports_each_step_and_the_release() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let first = untimed(drive(&canvas, &mut state, &arrow_up(), knob));
    assert_eq!(
        first.first(),
        Some(&CanvasEvent::ArrowStep { at: epoch() }),
        "{first:?}"
    );
    assert!(
        !first.iter().any(|e| matches!(e, CanvasEvent::GestureStart)),
        "an arrow sends no drag gesture: {first:?}"
    );
    let Event::Key(KeyEvent::Pressed {
        key, modifiers, text, ..
    }) = arrow_up()
    else {
        unreachable!()
    };
    let again = Event::Key(KeyEvent::Pressed {
        key,
        modifiers,
        text,
        repeat: true,
    });
    let repeat = untimed(drive(&canvas, &mut state, &again, knob));
    assert_eq!(
        repeat.first(),
        Some(&CanvasEvent::ArrowStep { at: epoch() }),
        "{repeat:?}"
    );
    let up = Event::Key(KeyEvent::Released { key });
    assert_eq!(
        untimed(drive(&canvas, &mut state, &up, knob)),
        vec![CanvasEvent::ArrowReleased { at: epoch() }]
    );
}

/// A right press and release at `at`, with no move in between.
fn right_click(canvas: &CanvasModel<'_>, state: &mut CanvasInput, at: Point) -> Vec<CanvasEvent> {
    let mut events = drive(canvas, state, &click(PointerButton::Right), at);
    events.extend(drive(
        canvas,
        state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Right)),
        at,
    ));
    events
}

/// Right-clicking an output opens the probe menu at that output
/// (panels.md, Add scope and Add meter).
#[test]
fn right_click_on_an_output_opens_the_probe_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &selected);
    let output = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let port = scene.port_position(output).unwrap();
    let events = right_click(&canvas, &mut CanvasInput::default(), port);
    assert_eq!(
        events,
        vec![CanvasEvent::ProbeMenu {
            output,
            world: port,
            at: port,
            area: Rect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
        }]
    );
}

/// Right-clicking a cable opens the menu for the cable's source, at the
/// point clicked.
#[test]
fn right_click_on_a_cable_opens_the_menu_for_its_source() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &selected);
    let from = scene.port_position(PortRef {
        module: 2,
        output: true,
        index: 0,
    });
    let to = scene.port_position(PortRef {
        module: 3,
        output: false,
        index: 0,
    });
    let [a, b, c, d] = crate::scene::cable_curve(from.unwrap(), to.unwrap());
    let middle = Point::new(
        (a.x + 3.0 * b.x + 3.0 * c.x + d.x) / 8.0,
        (a.y + 3.0 * b.y + 3.0 * c.y + d.y) / 8.0,
    );
    let events = right_click(&canvas, &mut CanvasInput::default(), middle);
    assert!(
        matches!(
            events.as_slice(),
            [CanvasEvent::ProbeMenu { output: PortRef { module: 2, output: true, index: 0 }, world, .. }]
                if *world == middle
        ),
        "{events:?}"
    );
}

/// A right-drag pans and opens no menu, even if it ends where it began.
#[test]
fn right_drag_pans_and_opens_no_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &selected);
    let port = scene
        .port_position(PortRef {
            module: 1,
            output: true,
            index: 0,
        })
        .unwrap();
    let mut state = CanvasInput::default();
    let mut events = drive(&canvas, &mut state, &click(PointerButton::Right), port);
    let away = port + Vector::new(30.0, 0.0);
    let moved = Event::Pointer(PointerEvent::Moved);
    events.extend(drive(&canvas, &mut state, &moved, away));
    let back = Event::Pointer(PointerEvent::Moved);
    events.extend(drive(&canvas, &mut state, &back, port));
    events.extend(drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Right)),
        port,
    ));
    assert!(events.is_empty(), "{events:?}");
}

/// Letting go of another button during a right click opens no menu and
/// keeps the click, so the right release still opens it.
#[test]
fn only_the_right_release_opens_the_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &selected);
    let port = scene
        .port_position(PortRef {
            module: 1,
            output: true,
            index: 0,
        })
        .unwrap();
    let mut state = CanvasInput::default();
    let pressed = drive(&canvas, &mut state, &click(PointerButton::Right), port);
    let left = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Left)),
        port,
    );
    assert!(pressed.is_empty() && left.is_empty(), "{left:?}");
    let right = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Right)),
        port,
    );
    assert!(matches!(right.as_slice(), [CanvasEvent::ProbeMenu { .. }]), "{right:?}");
}

/// A tile opens its menu; an input, empty canvas, and middle click do not.
#[test]
fn right_click_elsewhere_opens_no_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &selected);
    let input = scene
        .port_position(PortRef {
            module: 2,
            output: false,
            index: 0,
        })
        .unwrap();
    let tile = scene.module(1).unwrap().bounds().center();
    assert!(matches!(
        right_click(&canvas, &mut CanvasInput::default(), tile).as_slice(),
        [CanvasEvent::TileMenu { module: 1, .. }]
    ));
    for at in [input, Point::new(300.0, 500.0)] {
        let events = right_click(&canvas, &mut CanvasInput::default(), at);
        assert!(events.is_empty(), "{at:?}: {events:?}");
    }
    let output = scene
        .port_position(PortRef {
            module: 1,
            output: true,
            index: 0,
        })
        .unwrap();
    let mut state = CanvasInput::default();
    let mut events = drive(&canvas, &mut state, &click(PointerButton::Middle), output);
    events.extend(drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Middle)),
        output,
    ));
    assert!(events.is_empty(), "{events:?}");
}

/// Each edge of an XY pad and each handle of a range has its own parameter's
/// hover text (panel-widgets.md, XY pad, and Range).
#[test]
fn hover_text_follows_the_part_under_the_pointer() {
    let parts = || {
        Box::new([
            crate::scene::PartText {
                label: "First".to_string(),
                hover: HoverText::new("First", "The first value.", "first"),
            },
            crate::scene::PartText {
                label: "Second".to_string(),
                hover: HoverText::new("Second", "The second value.", "second"),
            },
        ])
    };
    let mut pad = module(1, 0.0);
    pad.panel = PanelView {
        width: 6,
        height: 6,
        widgets: vec![widget(WidgetViewKind::XyPad { parts: parts() }, 0)],
    };
    let mut range = module(2, 600.0);
    range.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Range {
                vertical: false,
                parts: parts(),
            },
            0,
        )],
    };
    let scene = Scene::new(vec![pad, range], Vec::new());
    let mut values = PanelValues::new();
    values.set(
        2,
        0,
        WidgetValue {
            position: 0.1,
            second: Some(crate::controls::PartValue {
                position: 0.9,
                ..Default::default()
            }),
            ..WidgetValue::default()
        },
    );
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none).values(&values);
    let at = |point: Point| CanvasInput {
        hover: Some(scene.hit(point, 8.0)),
        cursor: Some(point),
        ..CanvasInput::default()
    };

    let area = xy_area(scene.module(1).unwrap().widget_bounds(0).unwrap());
    let bottom = at(Point::new(area.center_x(), area.y + area.height - 2.0));
    assert_eq!(canvas.hover_target(&bottom), Some(HoverTarget::Control(1, 0)));
    let left = at(Point::new(area.x + 2.0, area.center_y()));
    assert_eq!(canvas.hover_target(&left), Some(HoverTarget::Second(1, 0)));
    let (label, hover, _, _) = canvas.hover_content(HoverTarget::Second(1, 0)).unwrap();
    assert_eq!((label, hover.description.as_str()), ("Second", "The second value."));
    let (label, _, _, _) = canvas.hover_content(HoverTarget::Control(1, 0)).unwrap();
    assert_eq!(label, "First");

    let rect = scene.module(2).unwrap().widget_bounds(0).unwrap();
    let low = at(crate::controls::range_point(rect, false, 0.1));
    assert_eq!(canvas.hover_target(&low), Some(HoverTarget::Control(2, 0)));
    let high = at(crate::controls::range_point(rect, false, 0.9));
    assert_eq!(canvas.hover_target(&high), Some(HoverTarget::Second(2, 0)));
}

/// Hiding a range ends the keyboard's inside and any grab at once, even if it
/// is shown again before the next key press (panel-widgets.md, rule 4).
#[test]
fn hiding_a_range_ends_the_inside() {
    let tile = |hidden: bool| {
        let mut range = widget(
            WidgetViewKind::Range {
                vertical: false,
                parts: Box::default(),
            },
            0,
        );
        range.hidden = hidden;
        let mut view = module(1, 0.0);
        view.panel = PanelView {
            width: 6,
            height: 2,
            widgets: vec![range],
        };
        Scene::new(vec![view], Vec::new())
    };
    let (shown, hidden) = (tile(false), tile(true));
    let selected = BTreeSet::from([1]);
    let at = shown.module(1).unwrap().bounds().center();
    let moved = Event::Pointer(PointerEvent::Moved);
    let mut state = CanvasInput {
        focus: FocusState::at(
            1,
            0,
            false,
            FocusMode::RangePart(RangePart {
                part: 0,
                grab: Some(RangeGrab::Band),
            }),
        ),
        ..CanvasInput::default()
    };

    let canvas = CanvasModel::new(&shown, &selected);
    drive(&canvas, &mut state, &moved, at);
    assert!(state.focus.range_part().is_some(), "a shown range keeps its grab");

    let canvas = CanvasModel::new(&hidden, &selected);
    drive(&canvas, &mut state, &moved, at);
    assert!(state.focus.range_part().is_none(), "hiding ends it");

    let canvas = CanvasModel::new(&shown, &selected);
    let events = drive(&canvas, &mut state, &key_event(Key::Right, true), at);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, CanvasEvent::SetControl { .. })),
        "{events:?}"
    );
}

/// Space on a focused XY pad passes on to Run, as on a knob, and a key with no
/// use is not swallowed (panels.md, Keyboard without a mouse).
#[test]
fn a_focused_xy_pad_does_not_use_space() {
    let mut pad = module(1, 0.0);
    pad.panel = PanelView {
        width: 6,
        height: 6,
        widgets: vec![widget(WidgetViewKind::XyPad { parts: Box::default() }, 0)],
    };
    let scene = Scene::new(vec![pad], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput {
        focus: FocusState::at(1, 0, false, FocusMode::Browse),
        ..CanvasInput::default()
    };
    let at = scene.module(1).unwrap().bounds().center();
    let events = drive(&canvas, &mut state, &key_event(Key::Space, true), at);
    assert_eq!(events, vec![CanvasEvent::ToggleRun]);
}

/// The second press of a double-click, with the command modifier held.
fn command_key(named: Key) -> Event {
    let mut event = key_event(named, true);
    if let Event::Key(KeyEvent::Pressed { modifiers, .. }) = &mut event {
        *modifiers = command();
    }
    event
}

/// Drives one event and says whether the canvas captured it.
fn captured(canvas: &CanvasModel<'_>, state: &mut CanvasInput, event: &Event, at: Point) -> bool {
    let bounds = Rect::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut out = Reaction::default();
    canvas.handle(state, Some(event), Some(at), bounds, Instant::now(), &mut out);
    out.taken
}

/// A double-click on a tile's header opens the tile and starts no move,
/// so a pointer move after it drags nothing (sub-patches.md, Opening a
/// sub-patch).
#[test]
fn a_header_double_click_opens_the_tile_and_starts_no_move() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let tile = scene.module(1).unwrap().bounds();
    let header = Point::new(tile.center_x(), tile.y + HEADER * 0.5);
    let mut state = CanvasInput::default();
    let released = Event::Pointer(PointerEvent::Released(PointerButton::Left));
    let first = drive(&canvas, &mut state, &click(PointerButton::Left), header);
    assert_eq!(first, vec![CanvasEvent::GestureStart], "a single press still moves");
    drive(&canvas, &mut state, &released, header);
    let second = drive(&canvas, &mut state, &click(PointerButton::Left), header);
    assert_eq!(second, vec![CanvasEvent::OpenTile(1)]);
    assert!(matches!(state.interaction, Interaction::Idle), "no drag is left open");
    let away = header + Vector::new(100.0, 100.0);
    let moved = drive(&canvas, &mut state, &Event::Pointer(PointerEvent::Moved), away);
    assert!(moved.is_empty(), "{moved:?}");
    let up = drive(&canvas, &mut state, &released, away);
    assert!(up.is_empty(), "{up:?}");
}

/// A double-click below the header keeps today's behaviour: the second
/// press starts a move like the first.
#[test]
fn a_double_click_on_the_tile_body_opens_nothing() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let tile = scene.module(1).unwrap().bounds();
    let body = tile.center();
    assert!(body.y >= tile.y + HEADER, "the centre is below the header");
    let mut state = CanvasInput::default();
    let released = Event::Pointer(PointerEvent::Released(PointerButton::Left));
    drive(&canvas, &mut state, &click(PointerButton::Left), body);
    drive(&canvas, &mut state, &released, body);
    let second = drive(&canvas, &mut state, &click(PointerButton::Left), body);
    assert_eq!(second, vec![CanvasEvent::GestureStart]);
    assert!(matches!(state.interaction, Interaction::Moving { .. }));
}

/// A pan reports the view once, when the drag ends, never per move.
#[test]
fn the_end_of_a_pan_drag_reports_the_camera_once() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut state = CanvasInput::default();
    let start = Point::new(400.0, 300.0);
    let mut events = drive(&canvas, &mut state, &click(PointerButton::Middle), start);
    for step in 1..=3 {
        let to = start + Vector::new(10.0 * step as f32, 5.0 * step as f32);
        events.extend(drive(&canvas, &mut state, &Event::Pointer(PointerEvent::Moved), to));
    }
    assert!(events.is_empty(), "nothing before the release: {events:?}");
    let end = start + Vector::new(30.0, 15.0);
    let released = drive(
        &canvas,
        &mut state,
        &Event::Pointer(PointerEvent::Released(PointerButton::Middle)),
        end,
    );
    assert_eq!(released, vec![CanvasEvent::Camera(state.camera)]);
    assert_eq!(state.camera.origin, Point::new(-30.0, -15.0));
}

/// A wheel zoom step and the frame key both report the view.
#[test]
fn a_zoom_step_and_a_frame_report_the_camera() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = CanvasModel::new(&scene, &none);
    let mut state = CanvasInput::default();
    let at = Point::new(200.0, 150.0);
    let wheel = Event::Pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y: 1.0 }));
    let zoomed = drive(&canvas, &mut state, &wheel, at);
    assert_eq!(zoomed, vec![CanvasEvent::Camera(state.camera)]);
    assert!(state.camera.zoom > 1.0);
    let framed = drive(&canvas, &mut state, &key_event(Key::Home, true), at);
    assert_eq!(framed, vec![CanvasEvent::Camera(state.camera)]);
}

/// A new level takes the camera the app kept for it, and frames the scene
/// when the app has none; both say which view is now in use, and report
/// the lights on screen again, as a new patch does. The same key again
/// changes nothing.
#[test]
fn a_level_change_adopts_its_camera_or_frames_the_scene() {
    let cameras = |events: Vec<CanvasEvent>| -> Vec<CanvasEvent> {
        events
            .into_iter()
            .filter(|e| matches!(e, CanvasEvent::Camera(_)))
            .collect()
    };
    let scene = chain();
    let none = BTreeSet::new();
    let redraw = Event::Redraw;
    let kept = Camera {
        origin: Point::new(-120.0, 80.0),
        zoom: 1.5,
    };
    let shown = || CanvasInput {
        framed_key: Some(0),
        level_key: Some(0),
        reported_lights: Some(Vec::new()),
        reported_full_traces: Some(true),
        ..CanvasInput::default()
    };
    let adopt = CanvasModel::new(&scene, &none).level(1, Some(kept));
    let mut state = shown();
    let events = drive(&adopt, &mut state, &redraw, Point::ORIGIN);
    assert_eq!(state.camera, kept);
    assert!(events.contains(&CanvasEvent::VisibleDisplays(Vec::new())), "{events:?}");
    assert_eq!(cameras(events), vec![CanvasEvent::Camera(kept)]);
    assert_eq!(state.level_key, Some(1));
    let again = drive(&adopt, &mut state, &redraw, Point::ORIGIN);
    assert!(again.is_empty(), "{again:?}");

    let first_visit = CanvasModel::new(&scene, &none).level(2, None);
    let mut state = shown();
    state.camera = kept;
    let events = drive(&first_visit, &mut state, &redraw, Point::ORIGIN);
    let mut framed = kept;
    framed.frame_readable(scene.bounds().unwrap(), Size::new(800.0, 600.0), READABLE_ZOOM);
    assert_eq!(state.camera, framed);
    assert_eq!(cameras(events), vec![CanvasEvent::Camera(framed)]);
}

/// Ctrl+Enter (Cmd+Enter on macOS) is the app's shortcut to open a
/// sub-patch, so the canvas leaves it alone. Plain Enter still focuses the
/// first control of the selected tile.
#[test]
fn command_enter_is_left_to_the_app() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = CanvasModel::new(&scene, &selected);
    let at = scene.module(1).unwrap().bounds().center();
    let mut state = CanvasInput::default();
    assert!(!captured(&canvas, &mut state, &command_key(Key::Enter), at));
    assert_eq!(state.focus.control(), None);
    let plain = drive(&canvas, &mut state, &key_event(Key::Enter, true), at);
    assert!(plain.is_empty(), "{plain:?}");
    assert_eq!(state.focus.control(), Some((1, 0)));
    // With a control focused, Ctrl+Enter is still not the canvas's.
    assert!(!captured(&canvas, &mut state, &command_key(Key::Enter), at));
    assert_eq!(state.focus.control(), Some((1, 0)));
}

/// The moment for a test: the pointer at `pointer`, a canvas of 800 by 600
/// at the window's corner, and the clock at `now`.
fn moment(pointer: Option<Point>, now: Instant) -> Moment {
    Moment {
        pointer,
        bounds: Rect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
        now,
    }
}

/// `handle` answers with what the event led to: here a press on a tile
/// starts a move, so the canvas reports it and takes the input.
#[test]
fn handle_reports_what_an_event_led_to() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let mut input = CanvasInput::default();
    let now = Instant::now();
    let tile = scene.module(1).expect("the module").bounds();
    let at = input
        .view(moment(None, now).bounds)
        .to_window(Point::new(tile.center_x(), tile.y + 10.0));
    input.handle(&model, &Event::Pointer(PointerEvent::Moved), moment(Some(at), now));
    let reaction = input.handle(&model, &click(PointerButton::Left), moment(Some(at), now));
    assert!(reaction.taken, "a press on a tile is the canvas's");
    assert!(!reaction.events.is_empty(), "a press on a tile reports something");
    assert_eq!(input.pointer(&model).hover, Some(Hit::Module(1)));
}

/// `handle` and `pass` both ask for the redraw at an arrow group's
/// deadline, and `pass` reports nothing and takes nothing.
#[test]
fn handle_and_pass_both_keep_the_deadline() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let now = Instant::now();
    let due = now + Duration::from_millis(400);
    let model = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Open { due });
    let mut input = CanvasInput::default();
    // The first redraw frames the patch and asks for a frame of its own.
    input.handle(&model, &Event::Redraw, moment(None, now));
    let handled = input.handle(&model, &Event::Redraw, moment(None, now));
    assert!(!handled.redraw, "already framed, so nothing changed");
    assert_eq!(input.next_deadline(&model), Some(due));
    let passed = input.pass(&model, moment(None, now));
    assert_eq!(input.next_deadline(&model), Some(due));
    assert!(!passed.redraw);
    assert!(passed.events.is_empty(), "{:?}", passed.events);
    assert!(!passed.taken);
}

/// The cursor is the plain arrow off the canvas, and says what a press
/// would do on it.
#[test]
fn the_cursor_follows_what_is_under_the_pointer() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let mut input = CanvasInput::default();
    let now = Instant::now();
    assert_eq!(input.cursor(&model, false), CursorShape::Default);
    let tile = scene.module(1).expect("the module").bounds();
    let at = input
        .view(moment(None, now).bounds)
        .to_window(Point::new(tile.center_x(), tile.y + 10.0));
    input.handle(&model, &Event::Pointer(PointerEvent::Moved), moment(Some(at), now));
    assert_eq!(input.cursor(&model, true), CursorShape::Grab, "a tile can be picked up");
    assert_eq!(
        input.cursor(&model, false),
        CursorShape::Default,
        "the pointer is elsewhere"
    );
    input.handle(&model, &click(PointerButton::Left), moment(Some(at), now));
    assert_eq!(
        input.cursor(&model, false),
        CursorShape::Grabbing,
        "a drag keeps its cursor anywhere"
    );
}

/// A double-click is judged by the time passed in, not by the clock: two
/// presses in the same place count only when they are close in time.
#[test]
fn a_double_click_is_judged_by_the_time_given() {
    let at = Point::new(10.0, 10.0);
    let first = Instant::now();
    assert!(double_click(Some((first, at)), at, first + Duration::from_millis(100)));
    assert!(!double_click(Some((first, at)), at, first + DOUBLE_CLICK));
    assert!(!double_click(None, at, first));
    let far = Point::new(10.0 + DOUBLE_CLICK_SLOP, 10.0);
    assert!(!double_click(
        Some((first, at)),
        far,
        first + Duration::from_millis(100)
    ));
}

/// The view, the pointer report, and the typed field report describe a new
/// canvas as empty.
#[test]
fn a_new_canvas_reports_nothing_under_the_pointer_and_no_field() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let input = CanvasInput::default();
    let bounds = Rect::new(Point::new(20.0, 30.0), Size::new(800.0, 600.0));
    assert_eq!(input.view(bounds).bounds, bounds);
    let pointer = input.pointer(&model);
    assert_eq!((pointer.hover, pointer.pinned), (None, None));
    assert!(pointer.drag.is_none());
    assert!(input.typed().is_none());
}

/// The next deadline is the earlier of the arrow group closing and hover
/// text appearing, and there is none when neither is pending.
#[test]
fn the_next_deadline_is_the_earlier_of_what_is_pending() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let now = Instant::now();
    let hover_due = now + crate::hover::HOVER_DELAY;
    let mut input = CanvasInput::default();
    let idle = CanvasModel::new(&scene, &none);
    assert_eq!(input.next_deadline(&idle), None, "nothing pending");

    // Hover text waiting on a title, and no arrow group.
    input.hover_text.rest(Some(HoverTarget::Title(1)), now);
    assert_eq!(input.next_deadline(&idle), Some(hover_due));

    // An arrow group that closes sooner comes first.
    let sooner = hover_due - Duration::from_millis(1);
    let closing = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Open { due: sooner });
    assert_eq!(input.next_deadline(&closing), Some(sooner));

    // One that closes later leaves the hover text first.
    let later = hover_due + Duration::from_millis(1);
    let waiting = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Open { due: later });
    assert_eq!(input.next_deadline(&waiting), Some(hover_due));

    // A held arrow group has no deadline of its own.
    let held = CanvasModel::new(&scene, &none).edit_group(EditGroupView::Held);
    assert_eq!(input.next_deadline(&held), Some(hover_due));
}

/// A world rectangle lands in the window where its corners do: moved by
/// the canvas's place in the window and scaled by the zoom.
#[test]
fn a_world_rect_in_the_window() {
    let view = CanvasView {
        bounds: Rect::new(Point::new(100.0, 50.0), Size::new(800.0, 600.0)),
        camera: Camera::default(),
    };
    let world = Rect::new(Point::new(10.0, 20.0), Size::new(30.0, 40.0));
    let found = view.rect_to_window(world);
    let corner = view.to_window(Point::new(10.0, 20.0));
    let far = view.to_window(Point::new(40.0, 60.0));
    assert_eq!((found.x, found.y), (corner.x, corner.y));
    assert!((found.x + found.width - far.x).abs() < 1e-4);
    assert!((found.y + found.height - far.y).abs() < 1e-4);
}

/// Sends one event through the canvas's public entry, as a host does, and
/// gives what it reported.
fn host_event(canvas: &CanvasModel<'_>, state: &mut CanvasInput, event: &Event) -> Vec<CanvasEvent> {
    let moment = Moment {
        pointer: Some(Point::new(700.0, 500.0)),
        bounds: Rect::new(Point::ORIGIN, Size::new(800.0, 600.0)),
        now: Instant::now(),
    };
    state.handle(canvas, event, moment).events
}

/// The canvas reports a focus change once, when focus lands on a control,
/// moves to another, and leaves. An event that keeps the focus where it is
/// reports none.
#[test]
fn a_focus_change_is_reported_once() {
    let mut tile = module(2, 0.0);
    tile.panel = panel();
    let scene = Scene::new(vec![tile], Vec::new());
    let selected = BTreeSet::from([2]);
    let canvas = CanvasModel::new(&scene, &selected);
    let mut state = CanvasInput::default();
    assert_eq!(state.focused_control(), None);

    let enter = host_event(&canvas, &mut state, &key_event(Key::Enter, true));
    assert_eq!(enter, vec![CanvasEvent::FocusChanged(Some((2, 0)))]);
    assert_eq!(state.focused_control(), Some((2, 0)));

    let tab = host_event(&canvas, &mut state, &key_event(Key::Tab, true));
    assert_eq!(tab, vec![CanvasEvent::FocusChanged(Some((2, 2)))]);

    let up = host_event(&canvas, &mut state, &key_event(Key::Enter, false));
    assert!(
        !up.iter().any(|event| matches!(event, CanvasEvent::FocusChanged(_))),
        "the focus stayed"
    );

    let escape = host_event(&canvas, &mut state, &key_event(Key::Escape, true));
    assert!(escape.contains(&CanvasEvent::FocusChanged(None)));
    assert_eq!(state.focused_control(), None);
}

/// A focus request moves keyboard focus when its key changes, and the
/// change is reported. It does nothing when the canvas first appears, for
/// a light, a hidden control, or a module that is gone, and it never
/// changes the selection.
#[test]
fn a_focus_request_moves_focus_when_its_key_changes() {
    let mut tile = module(2, 0.0);
    tile.panel = panel();
    tile.panel.widgets[2].hidden = true;
    let scene = Scene::new(vec![tile], Vec::new());
    let none = BTreeSet::new();
    let mut state = CanvasInput::default();
    let at = |key, control| CanvasModel::new(&scene, &none).focus_request(key, control);

    // The first redraw only notes the key.
    let first = host_event(&at(5, Some((2, 0))), &mut state, &Event::Redraw);
    assert!(!first.iter().any(|event| matches!(event, CanvasEvent::FocusChanged(_))));
    assert_eq!(state.focused_control(), None);

    let moved = host_event(&at(6, Some((2, 0))), &mut state, &Event::Redraw);
    assert!(moved.contains(&CanvasEvent::FocusChanged(Some((2, 0)))));
    assert!(
        moved.contains(&CanvasEvent::Select(vec![2])),
        "its tile is selected, as a press on the control selects it"
    );

    // With its tile already the one selected, nothing is selected again.
    let chosen = BTreeSet::from([2]);
    let again = CanvasModel::new(&scene, &chosen).focus_request(60, Some((2, 0)));
    state.focus.clear();
    let moved = host_event(&again, &mut state, &Event::Redraw);
    assert!(moved.contains(&CanvasEvent::FocusChanged(Some((2, 0)))));
    assert!(!moved.iter().any(|event| matches!(event, CanvasEvent::Select(_))));

    // The same key again asks for nothing.
    state.focus.clear();
    let same = host_event(&again, &mut state, &Event::Redraw);
    assert!(!same.iter().any(|event| matches!(event, CanvasEvent::FocusChanged(_))));

    // A light, a hidden control, a missing module, and no control at all.
    for (key, control) in [(7, Some((2, 1))), (8, Some((2, 2))), (9, Some((4, 0))), (10, None)] {
        let _ = host_event(&at(key, control), &mut state, &Event::Redraw);
        assert_eq!(state.focused_control(), None, "{control:?}");
    }
}
